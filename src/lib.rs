#![deny(unsafe_code)]
//! `aos-pet` — a virtual pet that lives in the capsule sandbox.
//!
//! The capsule is the referee: every stat lives in principal-scoped KV and
//! decays against the real wall clock, so the model cannot invent, freeze or
//! fake the pet's condition. All mutation happens through typed tool calls.

mod ailment;
mod art;
mod battle;
mod behaviour;
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
use model::{AlertKind, Decoded, Pet, KV_CORRUPT, KV_KEY};

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
pub struct AilmentView {
    /// Machine-readable: famine / grime / gloom.
    pub kind: String,
    /// How it looks to the owner.
    pub label: String,
    /// The one thing that actually fixes it.
    pub remedy: String,
}

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
    /// Which ailments are active, and what actually cures each. `sick` alone
    /// cannot tell a client whether to feed, wash or play.
    pub ailments: Vec<AilmentView>,
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
pub struct BattleView {
    pub opponent: String,
    pub taunt: String,
    /// Blow by blow, in order — a battle is an event, and events get retold.
    pub log: Vec<String>,
    pub won: bool,
    /// True when nobody fell and it was decided on remaining condition.
    pub on_points: bool,
    pub my_hp_left: u16,
    pub foe_hp_left: u16,
    pub victories: u32,
    pub message: String,
    pub pet: PetView,
}

#[derive(Debug, Serialize)]
pub struct MomentsView {
    pub name: String,
    /// How many distinct moments have been witnessed, out of how many exist.
    pub seen_count: u16,
    pub total: u16,
    /// What is happening right this second, if anything.
    pub now: Option<String>,
    pub seen: Vec<String>,
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

// Stats are f64 internally; gains and costs stay integer constants and are
// converted at the point of use.
fn stat_add(stat: f64, delta: u8) -> f64 {
    (stat + f64::from(delta)).clamp(0.0, 100.0)
}

fn stat_sub(stat: f64, delta: u8) -> f64 {
    (stat - f64::from(delta)).clamp(0.0, 100.0)
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
/// Fresh host entropy for scheduling the next moment. Falls back to a
/// clock-derived value only if the host refuses — a scheduled moment matters
/// less than the pet continuing to work.
fn fresh_seed(now: u64) -> u32 {
    let mut buf = [0u8; 4];
    match getrandom::fill(&mut buf) {
        Ok(()) => u32::from_le_bytes(buf),
        Err(_) => (now ^ (now >> 17)) as u32,
    }
}

/// Advance decay and the pet's own behaviour, recording what happened.
fn tick_pet(pet: &mut Pet, now: u64, cfg: &Config) {
    for kind in decay::advance(pet, now, cfg) {
        log::info(format!("[aos-pet] {}", kind.message(&pet.name)));
        pet.push_alert(kind, now);
    }
    for (kind, message) in behaviour::update(pet, now, cfg, fresh_seed(now)) {
        log::info(format!("[aos-pet] {message}"));
        pet.push_alert_with(kind, now, message);
    }
}

/// Read the stored pet record, turning every non-happy shape into an
/// actionable error. `Ok(None)` means no record exists at all.
///
/// This is the single place a bad record is translated for the player: a raw
/// serde error out of every tool would brick the capsule with no visible way
/// back, and a record from a newer build must not be reinterpreted under this
/// build's rules. Both errors name `pet_adopt` replace=true as the way out.
fn load_pet() -> Result<Option<Pet>, SysError> {
    let Some(bytes) = kv::get_bytes_opt(KV_KEY)? else {
        return Ok(None);
    };
    match model::decode_stored(&bytes) {
        Decoded::Pet(pet) => Ok(Some(pet)),
        Decoded::Newer(v) => Err(SysError::ApiError(format!(
            "Your pet record was saved by a newer capsule (record v{v}, this build reads \
             up to v{}). Upgrade the capsule, or call pet_adopt with replace=true to start over.",
            model::STATE_VERSION
        ))),
        Decoded::Corrupt(_) => Err(SysError::ApiError(
            "Your pet's saved record is unreadable. Call pet_adopt with replace=true to \
             start over — the broken record will be kept aside, not destroyed."
                .to_string(),
        )),
    }
}

/// Load the pet and bring it up to date. Player-facing only — the watchdog
/// deliberately does not come through here, so that witnessing a moment
/// requires somebody to actually look.
fn current(now: u64, cfg: &Config) -> Result<Pet, SysError> {
    let mut pet = load_pet()?.ok_or_else(|| {
        SysError::ApiError(
            "You have no pet yet. Call pet_adopt with a name to adopt one.".to_string(),
        )
    })?;
    tick_pet(&mut pet, now, cfg);

    if let Some(key) = pet
        .moment
        .as_ref()
        .and_then(moment::Active::def)
        .map(|d| d.key)
    {
        pet.witness(key);
    }
    Ok(pet)
}

fn view(pet: &Pet, now: u64, message: impl Into<String>) -> PetView {
    PetView {
        name: pet.name.clone(),
        mood: render::mood_name(pet).to_string(),
        // The outside world sees whole points; fractions stay internal.
        fullness: render::pt(pet.fullness),
        happiness: render::pt(pet.happiness),
        energy: render::pt(pet.energy),
        cleanliness: render::pt(pet.cleanliness),
        sleeping: pet.sleeping,
        sick: pet.sick,
        ailments: ailment::active(pet)
            .into_iter()
            .map(|a| AilmentView {
                kind: a.key().to_string(),
                label: a.label().to_string(),
                remedy: a.remedy().to_string(),
            })
            .collect(),
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

/// Raise the recovery alert when a cure has just cleared the last ailment.
///
/// This cannot live in `decay::advance`: that samples `sick` before a tool
/// runs, so a cure applied afterwards is invisible to it. It cannot live in
/// `commit` either, since only the cure paths know the before-state.
fn note_recovery(pet: &mut Pet, was_ill: bool, now: u64) {
    if was_ill && !pet.sick {
        pet.push_alert(AlertKind::Recovered, now);
        log::info(format!("[aos-pet] {} recovered", pet.name));
    }
}

/// True when nothing a player could notice has changed. Stats are compared at
/// player-visible (rounded) resolution: the raw f64s drift on every 5 s tick,
/// so comparing them directly would make every tick look like a change.
fn stats_unchanged(a: &Pet, b: &Pet) -> bool {
    render::pt(a.fullness) == render::pt(b.fullness)
        && render::pt(a.happiness) == render::pt(b.happiness)
        && render::pt(a.energy) == render::pt(b.energy)
        && render::pt(a.cleanliness) == render::pt(b.cleanliness)
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
        // Decode by hand instead of going through `load_pet`: adopt IS the
        // recovery path for a bad record, so it must not die on the very
        // error it exists to fix. A corrupt or too-new record still counts
        // as "there is a pet" and needs the same explicit replace consent.
        if let Some(bytes) = kv::get_bytes_opt(KV_KEY)? {
            match model::decode_stored(&bytes) {
                Decoded::Pet(existing) if !args.replace => {
                    return Err(SysError::ApiError(format!(
                        "You already care for {}. Pass replace=true to start over.",
                        existing.name
                    )));
                }
                Decoded::Newer(v) if !args.replace => {
                    return Err(SysError::ApiError(format!(
                        "A pet record from a newer capsule (v{v}) already exists. Upgrade \
                         the capsule to keep it, or pass replace=true to start over.",
                    )));
                }
                Decoded::Corrupt(_) if !args.replace => {
                    return Err(SysError::ApiError(
                        "An existing pet record is there but unreadable. Pass replace=true \
                         to adopt a new pet; the broken record will be kept aside."
                            .to_string(),
                    ));
                }
                Decoded::Corrupt(why) => {
                    // Park the raw bytes before overwriting: corruption must
                    // never silently destroy data. The copy stays under its
                    // own key for inspection or hand recovery, and the parse
                    // failure goes to the log — the one place the raw serde
                    // reason is useful — rather than at the player.
                    kv::set_bytes(KV_CORRUPT, &bytes)?;
                    log::info(format!(
                        "[aos-pet] unreadable record preserved under {KV_CORRUPT}: {why}"
                    ));
                }
                // replace=true over a readable record: plain abandonment.
                _ => {}
            }
        }
        // A new pet must not inherit the previous pet's in-flight guessing
        // round — its secret and spent energy belonged to a pet that no
        // longer exists. Idempotent, so the fresh-adopt path is free too.
        kv::delete(game::KV_GAME)?;

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
        // Note the clock is deliberately NOT stamped here: a refused meal must
        // leave readiness recharging, or spamming the bowl would pin the next
        // real feed — and with it the Famine cure — at nearly zero value.
        //
        // A pet convalescing from Famine is exempt: its bar recovers long before
        // the illness does, so refusing here would tell the player "feed it
        // regularly until it recovers" and then refuse every meal — the advice
        // and the rule contradicting each other with no way out.
        let convalescing = ailment::active(&pet).contains(&ailment::Ailment::Famine);
        if economy::is_overserving(pet.fullness) && !convalescing {
            pet.happiness = stat_sub(pet.happiness, economy::OVERSERVE_PENALTY);
            let msg = format!("{} is already full and turns away from the bowl.", pet.name);
            return commit(&pet, now, msg);
        }

        let was_ill = pet.sick;
        let ready = economy::readiness(pet.last_fed_ms, now, cfg.feed_ideal_hours, cfg.scale);
        pet.fullness = stat_add(pet.fullness, FEED_GAIN);
        pet.happiness = stat_add(pet.happiness, economy::payoff(FEED_JOY, ready));
        pet.last_fed_ms = now;
        // Feeding is the cure for hunger-sickness, not medicine.
        ailment::apply_remedy(&mut pet, ailment::Ailment::Famine, ready);
        pet.sick = ailment::is_ill(&pet);
        note_recovery(&mut pet, was_ill, now);

        let msg = format!(
            "{} munches happily. Fullness is now {}.{}",
            pet.name,
            render::pt(pet.fullness),
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

        // Gloom is cured by company, so a gloomy pet must always be allowed to
        // play — refusing on a blanket `sick` flag would lock the player out of
        // the only remedy that works. Physical illness still says no.
        if let Some(a) = ailment::blocks_play(&pet) {
            return Err(SysError::ApiError(format!(
                "{} is {} and cannot romp about. {}.",
                pet.name,
                a.label(),
                a.remedy()
            )));
        }
        if pet.energy < f64::from(PLAY_ENERGY_COST) {
            let msg = format!("{} is too tired to play — it needs rest.", pet.name);
            return commit(&pet, now, msg);
        }
        let was_ill = pet.sick;
        let ready = economy::readiness(pet.last_played_ms, now, cfg.play_ideal_hours, cfg.scale);
        pet.happiness = stat_add(pet.happiness, economy::payoff(PLAY_GAIN, ready));
        pet.energy = stat_sub(pet.energy, PLAY_ENERGY_COST);
        pet.last_played_ms = now;
        // Company is the only thing that lifts gloom — medicine cannot.
        ailment::apply_remedy(&mut pet, ailment::Ailment::Gloom, ready);
        pet.sick = ailment::is_ill(&pet);
        note_recovery(&mut pet, was_ill, now);

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

        // As with feeding: a refused wash must not restart the clock, and a pet
        // still carrying Grime must never be turned away from the water.
        let convalescing = ailment::active(&pet).contains(&ailment::Ailment::Grime);
        if economy::is_overserving(pet.cleanliness) && !convalescing {
            pet.happiness = stat_sub(pet.happiness, economy::OVERSERVE_PENALTY);
            let msg = format!("{} is already spotless and squirms away from the water.", pet.name);
            return commit(&pet, now, msg);
        }

        let was_ill = pet.sick;
        let ready = economy::readiness(pet.last_cleaned_ms, now, cfg.clean_ideal_hours, cfg.scale);
        pet.cleanliness = 100.0;
        pet.happiness = stat_add(pet.happiness, economy::payoff(CLEAN_JOY, ready));
        pet.last_cleaned_ms = now;
        ailment::apply_remedy(&mut pet, ailment::Ailment::Grime, ready);
        pet.sick = ailment::is_ill(&pet);
        note_recovery(&mut pet, was_ill, now);

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

        // Dosing on any path, including the no-op one, so repeat presses cannot
        // farm the clock by bouncing off the healthy early return.
        let ready = economy::readiness(pet.last_healed_ms, now, cfg.heal_ideal_hours, cfg.scale);
        pet.last_healed_ms = now;

        let before = ailment::active(&pet);
        if before.is_empty() {
            let msg = format!("{} is perfectly healthy.", pet.name);
            return commit(&pet, now, msg);
        }

        ailment::apply_medicine(&mut pet, ready);
        pet.happiness = stat_add(pet.happiness, economy::payoff(HEAL_BOOST / 2, ready));
        pet.sick = ailment::is_ill(&pet);
        note_recovery(&mut pet, true, now);

        let remaining = ailment::active(&pet);
        let msg = if remaining.is_empty() {
            log::info(format!("[aos-pet] {} recovered", pet.name));
            format!("{} takes the medicine and perks up — recovered!", pet.name)
        } else {
            // Medicine is not a master key: say plainly what is wrong and what
            // would actually fix it, or the tool reads as broken.
            let what: Vec<&str> = remaining.iter().map(|a| a.label()).collect();
            let how: Vec<&str> = remaining.iter().map(|a| a.remedy()).collect();
            format!(
                "{} takes the medicine and settles a little, but it is still {}. \
                 Medicine only buys time — {}.",
                pet.name,
                what.join(" and "),
                how.join("; ")
            )
        };
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

        // Same rule as pet_play: games are a listed cure for Gloom, so a
        // blanket `sick` check here would gate the remedy behind the ailment it
        // is meant to lift — and send the player to pet_heal, which by design
        // cannot touch gloom at all.
        if let Some(a) = ailment::blocks_play(&pet) {
            return Err(SysError::ApiError(format!(
                "{} is {} and cannot concentrate on a game. {}.",
                pet.name,
                a.label(),
                a.remedy()
            )));
        }
        if pet.energy < f64::from(game::ENERGY_COST) {
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

        let was_ill = pet.sick;
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
                // A won game is earned attention, so it always counts in full.
                ailment::apply_remedy(&mut pet, ailment::Ailment::Gloom, 1.0);
                pet.sick = ailment::is_ill(&pet);
                note_recovery(&mut pet, was_ill, now);
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

    /// Pick a friendly scrap with a passing stranger. Fighting ability is
    /// derived from how well the pet has been looked after — there is nothing
    /// to train, so a fed, rested, clean pet simply wins more. Costs energy,
    /// and a tired pet will refuse.
    #[astrid::tool("pet_battle")]
    pub fn pet_battle(&self, _args: NoArgs) -> Result<BattleView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let mut pet = current(now, &cfg)?;
        refuse_if_asleep(&pet)?;

        // Unlike games, a scrap is not a cure for anything, so ANY ailment
        // still bars it — a miserable pet has no business in a fight either.
        // The refusal must name the real remedy though: sending a gloomy pet to
        // pet_heal would be advice that provably cannot work.
        if let Some(a) = ailment::active(&pet).first().copied() {
            return Err(SysError::ApiError(format!(
                "{} is {} and in no shape to scrap. {}.",
                pet.name,
                a.label(),
                a.remedy()
            )));
        }
        if pet.energy < f64::from(battle::MIN_ENERGY) {
            return Err(SysError::ApiError(format!(
                "{} is too tired to fight — it needs rest first.",
                pet.name
            )));
        }

        let mine = battle::stats_for(&pet, now);
        let report = battle::fight(&pet.name, mine, fresh_seed(now));

        pet.energy = stat_sub(pet.energy, battle::ENERGY_COST);
        let message = if report.won {
            pet.victories = pet.victories.saturating_add(1);
            pet.happiness = stat_add(pet.happiness, 20);
            format!(
                "{} sends {} packing{}!",
                pet.name,
                report.opponent,
                if report.on_points { " on points" } else { "" }
            )
        } else {
            // Losing costs only the energy already spent. Nothing is injured.
            format!(
                "{} comes off worse against {}, but shakes it off.",
                pet.name, report.opponent
            )
        };
        log::info(format!("[aos-pet] battle: won={} vs {}", report.won, report.opponent));
        save(&pet)?;

        Ok(BattleView {
            opponent: report.opponent,
            taunt: report.taunt,
            log: report.log,
            won: report.won,
            on_points: report.on_points,
            my_hp_left: report.my_hp_left,
            foe_hp_left: report.foe_hp_left,
            victories: pet.victories,
            pet: view(&pet, now, message.clone()),
            message,
        })
    }

    /// The collection: which rare moments this pet has been caught having, and
    /// how many are still out there. Moments only count when somebody looks —
    /// the watchdog can start one unattended, but seeing it is on you.
    #[astrid::tool("pet_moments")]
    pub fn pet_moments(&self, _args: NoArgs) -> Result<MomentsView, SysError> {
        let cfg = load_config();
        let now = now_ms()?;
        let pet = current(now, &cfg)?;
        save(&pet)?;

        let seen: Vec<String> = moment::MOMENTS
            .iter()
            .filter(|m| pet.seen_moments.iter().any(|k| k == m.key))
            .map(|m| m.label.to_string())
            .collect();

        Ok(MomentsView {
            name: pet.name.clone(),
            seen_count: seen.len() as u16,
            total: moment::MOMENTS.len() as u16,
            now: pet
                .moment
                .as_ref()
                .and_then(moment::Active::def)
                .map(|d| d.label.to_string()),
            seen,
        })
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
        // Only a readable record is tickable. A corrupt or too-new one is
        // the player's problem to fix through pet_adopt; failing here would
        // just have the kernel deny this handler every 5 seconds.
        let Some(bytes) = kv::get_bytes_opt(KV_KEY)? else {
            return Ok(());
        };
        let Decoded::Pet(mut pet) = model::decode_stored(&bytes) else {
            return Ok(());
        };

        let before = pet.clone();
        tick_pet(&mut pet, now, &cfg);

        // The cheap "nothing happened" skip has to account for everything the
        // player could notice, not just the four bars. A moment starting is
        // observable while every stat stays byte-identical, and the moment
        // schedule MUST be persisted — drop it and the next tick reschedules
        // from scratch, postponing moments forever.
        let unchanged = stats_unchanged(&before, &pet)
            && before.moment == pet.moment
            && before.sleeping == pet.sleeping
            && before.next_moment_ms == pet.next_moment_ms
            && before.alerts.len() == pet.alerts.len();
        if unchanged {
            // Leaving `last_seen` alone also keeps the maths honest: the next
            // tick measures one full span instead of rounding away many tiny ones.
            return Ok(());
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
        // Say nothing when there is no pet — and equally when the record is
        // unreadable or from a newer build: the prompt hook has no player to
        // hand an error to, and failing it would only poison prompt builds.
        let Some(bytes) = kv::get_bytes_opt(KV_KEY)? else {
            return Ok(());
        };
        let Decoded::Pet(mut pet) = model::decode_stored(&bytes) else {
            return Ok(());
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
        assert_eq!(stat_add(90.0, 30), 100.0);
        assert_eq!(stat_add(250.0, 30), 100.0);
        assert_eq!(stat_sub(10.0, 30), 0.0);
        assert_eq!(stat_sub(40.0, 15), 25.0);
    }

    #[test]
    fn stats_unchanged_ignores_bookkeeping_but_catches_real_change() {
        let a = Pet::new("Rex".into(), 0);

        // last_seen moving on its own is not a player-visible change.
        let mut only_clock = a.clone();
        only_clock.last_seen_ms = 999_999;
        assert!(stats_unchanged(&a, &only_clock));

        // Sub-point drift is invisible to the player and must not count,
        // or continuous decay would make every 5 s tick look like a change.
        let mut fractional = a.clone();
        fractional.fullness -= 0.2;
        assert!(stats_unchanged(&a, &fractional));

        for mutate in [
            (|p: &mut Pet| p.fullness -= 1.0) as fn(&mut Pet),
            |p: &mut Pet| p.happiness -= 1.0,
            |p: &mut Pet| p.energy -= 1.0,
            |p: &mut Pet| p.cleanliness -= 1.0,
            |p: &mut Pet| p.sick = true,
        ] {
            let mut b = a.clone();
            mutate(&mut b);
            assert!(!stats_unchanged(&a, &b), "a real change must be detected");
        }
    }
}
