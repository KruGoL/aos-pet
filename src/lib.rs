#![deny(unsafe_code)]
//! `aos-pet` — a virtual pet that lives in the capsule sandbox.
//!
//! The capsule is the referee: every stat lives in principal-scoped KV and
//! decays against the real wall clock, so the model cannot invent, freeze or
//! fake the pet's condition. All mutation happens through typed tool calls.

mod art;
mod config;
mod decay;
mod economy;
mod game;
mod moment;
mod model;
mod mood;
mod render;

use astrid_sdk::prelude::*;
use astrid_sdk::schemars;
use serde::{Deserialize, Serialize};

use config::Config;
use model::{AlertKind, Pet, KV_KEY};

const FEED_GAIN: u8 = 30;
const FEED_JOY: u8 = 5;
const CLEAN_JOY: u8 = 8;
const PLAY_GAIN: u8 = 25;
const PLAY_ENERGY_COST: u8 = 15;
const HEAL_BOOST: u8 = 20;

// ---------------------------------------------------------------- arguments

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct AdoptArgs {
    /// What to call the pet.
    pub name: String,
    /// Set true to abandon an existing pet and start over. Defaults to false.
    #[serde(default)]
    pub replace: bool,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct RenameArgs {
    /// The pet's new name. Everything else — age, stats, history — is kept.
    pub name: String,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct GuessArgs {
    /// Your guess, within the range the game reported when it started.
    pub value: u32,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SleepArgs {
    /// Set true to wake the pet up. Defaults to false, which puts it to sleep.
    #[serde(default)]
    pub wake: bool,
}

// ------------------------------------------------------------------ results

#[derive(Debug, Serialize)]
pub struct PetView {
    pub name: String,
    pub mood: String,
    pub fullness: u8,
    pub happiness: u8,
    pub energy: u8,
    pub cleanliness: u8,
    pub sleeping: bool,
    pub sick: bool,
    pub age_hours: u64,
    /// What just happened, in words.
    pub message: String,
    /// Ready-to-print art plus stat bars.
    pub display: String,
    /// One-line summary for a status bar or shell prompt.
    pub line: String,
    /// Severity for clients that colour their output: ok / warn / critical / resting.
    pub level: String,
    /// Both animation frames, for a viewer that redraws on a timer.
    pub frames: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AlertOut {
    pub at_ms: u64,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AlertsView {
    pub name: String,
    pub alerts: Vec<AlertOut>,
}

#[derive(Debug, Serialize)]
pub struct GameView {
    /// What just happened, phrased for the player.
    pub message: String,
    /// Whether a round is still in progress.
    pub active: bool,
    pub guesses_used: u32,
    pub guesses_left: u32,
    /// The playable range, e.g. "1-20".
    pub range: String,
    /// The pet, so callers can show it reacting.
    pub pet: PetView,
}

// ------------------------------------------------------------------ helpers

fn stat_add(stat: u8, delta: u8) -> u8 {
    stat.saturating_add(delta).min(100)
}

fn stat_sub(stat: u8, delta: u8) -> u8 {
    stat.saturating_sub(delta)
}

fn load_config() -> Config {
    match env::var_opt("decay_scale") {
        Ok(Some(raw)) => Config::default().with_scale_str(&raw),
        _ => Config::default(),
    }
}

fn now_ms() -> Result<u64, SysError> {
    let d = time::now()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| SysError::ApiError(format!("system clock predates the unix epoch: {e}")))?;
    Ok(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn save(pet: &Pet) -> Result<(), SysError> {
    kv::set_json(KV_KEY, pet)
}

/// A secret the model cannot predict. Real host entropy, never the clock — a
/// clock-derived number would be reproducible by anything that can read the
/// time, which defeats the whole point of hiding it.
fn pick_secret() -> Result<u32, SysError> {
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf)
        .map_err(|e| SysError::ApiError(format!("no randomness available: {e}")))?;
    Ok(game::secret_from_bytes(buf))
}

fn game_view(
    pet: &Pet,
    now: u64,
    message: impl Into<String>,
    active: bool,
    used: u32,
    left: u32,
) -> GameView {
    let message = message.into();
    GameView {
        pet: view(pet, now, message.clone()),
        message,
        active,
        guesses_used: used,
        guesses_left: left,
        range: format!("{}-{}", game::MIN, game::MAX),
    }
}

/// Load the pet and bring it up to date, recording any thresholds crossed
/// while nobody was looking. Every entry point goes through here.
fn current(now: u64, cfg: &Config) -> Result<Pet, SysError> {
    let mut pet = kv::get_json_opt::<Pet>(KV_KEY)?.ok_or_else(|| {
        SysError::ApiError(
            "You have no pet yet. Call pet_adopt with a name to adopt one.".to_string(),
        )
    })?;
    for kind in decay::advance(&mut pet, now, cfg) {
        log::info(format!("[aos-pet] {}", kind.message(&pet.name)));
        pet.push_alert(kind, now);
    }
    Ok(pet)
}

fn view(pet: &Pet, now: u64, message: impl Into<String>) -> PetView {
    PetView {
        name: pet.name.clone(),
        mood: render::mood_name(pet).to_string(),
        fullness: pet.fullness,
        happiness: pet.happiness,
        energy: pet.energy,
        cleanliness: pet.cleanliness,
        sleeping: pet.sleeping,
        sick: pet.sick,
        age_hours: pet.age_ms(now) / 3_600_000,
        message: message.into(),
        display: render::display(pet, 0, now),
        line: render::compact(pet),
        level: render::level(pet).to_string(),
        frames: render::frames_for(pet, now),
    }
}

/// Persist and describe. Used by every tool, including read-only ones — a
/// status check still advances the decay clock and must be saved.
fn commit(pet: &Pet, now: u64, message: impl Into<String>) -> Result<PetView, SysError> {
    save(pet)?;
    Ok(view(pet, now, message))
}

/// True when nothing a player could notice has changed.
fn stats_unchanged(a: &Pet, b: &Pet) -> bool {
    a.fullness == b.fullness
        && a.happiness == b.happiness
        && a.energy == b.energy
        && a.cleanliness == b.cleanliness
        && a.sick == b.sick
}

fn refuse_if_asleep(pet: &Pet) -> Result<(), SysError> {
    if pet.sleeping {
        return Err(SysError::ApiError(format!(
            "{} is asleep. Wake it first with pet_sleep wake=true.",
            pet.name
        )));
    }
    Ok(())
}

// ------------------------------------------------------------------ capsule

#[derive(Default)]
pub struct Capsule;

#[capsule]
impl Capsule {
    /// Adopt a virtual pet and give it a name. Each user has their own pet.
    /// Fails if a pet already exists unless `replace` is true.
    #[astrid::tool("pet_adopt")]
    pub fn pet_adopt(&self, args: AdoptArgs) -> Result<PetView, SysError> {
        let now = now_ms()?;
        if let Some(existing) = kv::get_json_opt::<Pet>(KV_KEY)? {
            if !args.replace {
                return Err(SysError::ApiError(format!(
                    "You already care for {}. Pass replace=true to start over.",
                    existing.name
                )));
            }
        }
        let pet = Pet::new(Pet::sanitize_name(&args.name), now);
        let greeting = format!("{} hops out of the box and looks up at you!", pet.name);
        log::info(format!("[aos-pet] adopted {}", pet.name));
        commit(&pet, now, greeting)
    }

    /// Look at your pet: mood, ASCII art and every stat bar. Stats are brought
    /// up to date with real elapsed time first, so this shows its true state.
    #[astrid::tool("pet_status")]
    pub fn pet_status(&self, _args: NoArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let pet = current(now, &cfg)?;
        let msg = format!("{} is {}.", pet.name, render::mood_name(&pet));
        commit(&pet, now, msg)
    }

    /// Feed the pet. Raises fullness and cheers it up slightly.
    #[astrid::tool("pet_feed")]
    pub fn pet_feed(&self, _args: NoArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;
        refuse_if_asleep(&pet)?;

        // Pressing food on a pet that does not want it is fussing, not care.
        if economy::is_overserving(pet.fullness) {
            pet.happiness = stat_sub(pet.happiness, economy::OVERSERVE_PENALTY);
            pet.last_fed_ms = now;
            let msg = format!("{} is already full and turns away from the bowl.", pet.name);
            return commit(&pet, now, msg);
        }

        let ready = economy::readiness(pet.last_fed_ms, now, cfg.feed_ideal_hours, cfg.scale);
        pet.fullness = stat_add(pet.fullness, FEED_GAIN);
        pet.happiness = stat_add(pet.happiness, economy::payoff(FEED_JOY, ready));
        pet.last_fed_ms = now;

        let msg = format!(
            "{} munches happily. Fullness is now {}.{}",
            pet.name,
            pet.fullness,
            economy::payoff_note(ready)
        );
        commit(&pet, now, msg)
    }

    /// Play with the pet. Raises happiness but costs energy. Refuses while the
    /// pet is ill or asleep.
    #[astrid::tool("pet_play")]
    pub fn pet_play(&self, _args: NoArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;
        refuse_if_asleep(&pet)?;

        if pet.sick {
            return Err(SysError::ApiError(format!(
                "{} is too ill to play. Try pet_heal first.",
                pet.name
            )));
        }
        if pet.energy < PLAY_ENERGY_COST {
            let msg = format!("{} is too tired to play — it needs rest.", pet.name);
            return commit(&pet, now, msg);
        }
        let ready = economy::readiness(pet.last_played_ms, now, cfg.play_ideal_hours, cfg.scale);
        pet.happiness = stat_add(pet.happiness, economy::payoff(PLAY_GAIN, ready));
        pet.energy = stat_sub(pet.energy, PLAY_ENERGY_COST);
        pet.last_played_ms = now;

        let msg = format!(
            "You play together. {} is delighted!{}",
            pet.name,
            economy::payoff_note(ready)
        );
        commit(&pet, now, msg)
    }

    /// Put the pet to sleep, or wake it with `wake=true`. Energy recovers while
    /// asleep and everything else decays more slowly.
    #[astrid::tool("pet_sleep")]
    pub fn pet_sleep(&self, args: SleepArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;

        let msg = if args.wake {
            if pet.sleeping {
                pet.sleeping = false;
                format!("{} stretches and wakes up.", pet.name)
            } else {
                format!("{} is already awake.", pet.name)
            }
        } else if pet.sleeping {
            format!("{} is already fast asleep.", pet.name)
        } else {
            pet.sleeping = true;
            format!("{} curls up and drifts off.", pet.name)
        };
        commit(&pet, now, msg)
    }

    /// Wash the pet. Restores cleanliness completely.
    #[astrid::tool("pet_clean")]
    pub fn pet_clean(&self, _args: NoArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;
        refuse_if_asleep(&pet)?;

        if economy::is_overserving(pet.cleanliness) {
            pet.happiness = stat_sub(pet.happiness, economy::OVERSERVE_PENALTY);
            pet.last_cleaned_ms = now;
            let msg = format!("{} is already spotless and squirms away from the water.", pet.name);
            return commit(&pet, now, msg);
        }

        let ready = economy::readiness(pet.last_cleaned_ms, now, cfg.clean_ideal_hours, cfg.scale);
        pet.cleanliness = 100;
        pet.happiness = stat_add(pet.happiness, economy::payoff(CLEAN_JOY, ready));
        pet.last_cleaned_ms = now;

        let msg = format!(
            "{} is scrubbed clean and smells lovely.{}",
            pet.name,
            economy::payoff_note(ready)
        );
        commit(&pet, now, msg)
    }

    /// Cure a sick pet. Neglect makes a pet ill, but it never dies — healing
    /// always works.
    #[astrid::tool("pet_heal")]
    pub fn pet_heal(&self, _args: NoArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;

        if !pet.sick {
            let msg = format!("{} is perfectly healthy.", pet.name);
            return commit(&pet, now, msg);
        }
        pet.sick = false;
        pet.neglect_ms = 0;
        pet.fullness = stat_add(pet.fullness, HEAL_BOOST);
        pet.happiness = stat_add(pet.happiness, HEAL_BOOST);
        pet.push_alert(AlertKind::Recovered, now);
        log::info(format!("[aos-pet] {} recovered", pet.name));
        let msg = format!(
            "{} takes the medicine and perks up. Illness cured!",
            pet.name
        );
        commit(&pet, now, msg)
    }

    /// Recent things that happened to the pet while you were away — when it got
    /// hungry, lonely or ill.
    #[astrid::tool("pet_alerts")]
    pub fn pet_alerts(&self, _args: NoArgs) -> Result<AlertsView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let pet = current(now, &cfg)?;
        save(&pet)?;
        Ok(AlertsView {
            name: pet.name.clone(),
            alerts: pet
                .alerts
                .iter()
                .map(|a| AlertOut {
                    at_ms: a.at_ms,
                    kind: format!("{:?}", a.kind).to_lowercase(),
                    message: a.message.clone(),
                })
                .collect(),
        })
    }

    /// Rename the pet, keeping its age, stats and history. Use this rather than
    /// adopting again — adopting replaces the pet and resets everything.
    #[astrid::tool("pet_rename")]
    pub fn pet_rename(&self, args: RenameArgs) -> Result<PetView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;

        let old = pet.name.clone();
        let new = Pet::sanitize_name(&args.name);
        if new == old {
            let msg = format!("{old} already answers to that name.");
            return commit(&pet, now, msg);
        }
        pet.name = new.clone();
        log::info(format!("[aos-pet] renamed {old} -> {new}"));
        let msg = format!("{old} will answer to {new} from now on.");
        commit(&pet, now, msg)
    }

    /// Play a guessing game with the pet. The capsule picks a secret number and
    /// keeps it inside the sandbox, so you and the agent both genuinely have to
    /// guess — winning cheers the pet up far more than plain play.
    #[astrid::tool("pet_game_start")]
    pub fn pet_game_start(&self, _args: NoArgs) -> Result<GameView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;
        refuse_if_asleep(&pet)?;

        if pet.sick {
            return Err(SysError::ApiError(format!(
                "{} is too ill for games. Try pet_heal first.",
                pet.name
            )));
        }
        if pet.energy < game::ENERGY_COST {
            save(&pet)?;
            let msg = format!("{} is too tired to play — it needs rest.", pet.name);
            return Ok(game_view(&pet, now, msg, false, 0, 0));
        }

        let g = game::Game::new(pick_secret()?, now);
        pet.energy = stat_sub(pet.energy, game::ENERGY_COST);
        save(&pet)?;
        kv::set_json(game::KV_GAME, &g)?;

        let msg = format!(
            "{} is thinking of a number between {} and {}. You have {} guesses — call pet_game_guess.",
            pet.name,
            game::MIN,
            game::MAX,
            game::MAX_GUESSES
        );
        Ok(game_view(&pet, now, msg, true, 0, game::MAX_GUESSES))
    }

    /// Make a guess in the running game. The capsule alone knows the answer, so
    /// the hints it returns are the only information anyone gets.
    #[astrid::tool("pet_game_guess")]
    pub fn pet_game_guess(&self, args: GuessArgs) -> Result<GameView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;

        let Some(mut g) = kv::get_json_opt::<game::Game>(game::KV_GAME)? else {
            return Err(SysError::ApiError(
                "No game in progress. Call pet_game_start to begin one.".to_string(),
            ));
        };

        let outcome = game::guess(&mut g, args.value);
        let warmth = game::warmth(g.secret, args.value);

        let msg = match outcome {
            game::Outcome::OutOfRange => {
                kv::set_json(game::KV_GAME, &g)?;
                save(&pet)?;
                let msg = format!(
                    "{} is only between {} and {} — that one does not count.",
                    args.value,
                    game::MIN,
                    game::MAX
                );
                return Ok(game_view(&pet, now, msg, true, g.guesses, g.guesses_left()));
            }
            game::Outcome::Won { guesses, reward } => {
                pet.happiness = stat_add(pet.happiness, reward);
                kv::delete(game::KV_GAME)?;
                save(&pet)?;
                let msg = format!(
                    "{} it is — guessed in {guesses}! {} is delighted (+{reward} happiness).",
                    args.value, pet.name
                );
                return Ok(game_view(&pet, now, msg, false, guesses, 0));
            }
            game::Outcome::Lost { secret } => {
                kv::delete(game::KV_GAME)?;
                save(&pet)?;
                let msg = format!(
                    "Out of guesses — {} was thinking of {secret}. Another round?",
                    pet.name
                );
                return Ok(game_view(&pet, now, msg, false, g.guesses, 0));
            }
            game::Outcome::Lower => format!("Lower than {} ({warmth}).", args.value),
            game::Outcome::Higher => format!("Higher than {} ({warmth}).", args.value),
        };

        kv::set_json(game::KV_GAME, &g)?;
        save(&pet)?;
        let msg = format!("{msg} {} guesses left.", g.guesses_left());
        Ok(game_view(&pet, now, msg, true, g.guesses, g.guesses_left()))
    }

    /// Kernel watchdog tick — fires roughly every 5 seconds while the daemon
    /// runs. This is how the pet notices it got hungry with nobody watching.
    ///
    /// Deliberately takes NO argument. The WIT record declares `timestamp-ms`,
    /// but the kernel actually publishes an empty object, so a handler with a
    /// required payload field fails to deserialize and is denied.
    #[astrid::interceptor("handle_watchdog_tick")]
    pub fn handle_watchdog_tick(&self) -> Result<(), SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let Some(mut pet) = kv::get_json_opt::<Pet>(KV_KEY)? else {
            return Ok(());
        };

        let before = pet.clone();
        let crossed = decay::advance(&mut pet, now, &cfg);
        if crossed.is_empty() && stats_unchanged(&before, &pet) {
            // Nothing observable happened, so skip the write. This also keeps
            // the maths honest: leaving `last_seen` alone means the next tick
            // measures one full span instead of rounding away many tiny ones.
            return Ok(());
        }
        for kind in &crossed {
            log::info(format!("[aos-pet] {}", kind.message(&pet.name)));
            pet.push_alert(*kind, now);
        }
        save(&pet)
    }

    /// Fold the pet's condition into the agent's system prompt, so the agent
    /// can raise it in conversation without being asked.
    ///
    /// Must be fast: the prompt builder stops collecting hook responses after
    /// ~250 ms. A KV read plus arithmetic comfortably fits.
    #[astrid::interceptor("on_before_prompt_build")]
    pub fn on_before_prompt_build(&self, payload: serde_json::Value) -> Result<(), SysError> {
        // Always take the reply topic from the payload — never build it.
        let Some(topic) = payload.get("response_topic").and_then(|v| v.as_str()) else {
            return Ok(());
        };
        let Some(mut pet) = kv::get_json_opt::<Pet>(KV_KEY)? else {
            return Ok(()); // no pet adopted — say nothing
        };

        let cfg = load_config();
        let now = now_ms()?;
        let _ = decay::advance(&mut pet, now, &cfg); // report the CURRENT state
        // Read-only path: deliberately no save, to keep this fast and avoid
        // fighting the tick for the same key.

        // NOTE the case asymmetry: the inbound payload is snake_case, but the
        // response fields the builder reads are camelCase.
        ipc::publish_json(
            topic,
            &serde_json::json!({ "appendSystemContext": render::prompt_section(&pet) }),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_helpers_saturate_at_both_ends() {
        assert_eq!(stat_add(90, 30), 100);
        assert_eq!(stat_add(250, 30), 100);
        assert_eq!(stat_sub(10, 30), 0);
        assert_eq!(stat_sub(40, 15), 25);
    }

    #[test]
    fn stats_unchanged_ignores_bookkeeping_but_catches_real_change() {
        let a = Pet::new("Rex".into(), 0);

        // last_seen moving on its own is not a player-visible change.
        let mut only_clock = a.clone();
        only_clock.last_seen_ms = 999_999;
        assert!(stats_unchanged(&a, &only_clock));

        for mutate in [
            (|p: &mut Pet| p.fullness -= 1) as fn(&mut Pet),
            |p: &mut Pet| p.happiness -= 1,
            |p: &mut Pet| p.energy -= 1,
            |p: &mut Pet| p.cleanliness -= 1,
            |p: &mut Pet| p.sick = true,
        ] {
            let mut b = a.clone();
            mutate(&mut b);
            assert!(!stats_unchanged(&a, &b), "a real change must be detected");
        }
    }
}
