//! What the pet does on its own: rare moments, and looking after itself.
//!
//! Everything here runs from the same single-shot entry point as decay, so no
//! rule may need to iterate per elapsed period — a pet untouched for a year
//! must cost the same as one checked a second ago. Transitions are therefore
//! applied to the *end* state of a span rather than simulated across it.
//!
//! Pure: the caller supplies fresh entropy, so tests are deterministic.

use crate::config::{Config, MS_PER_HOUR};
use crate::model::{AlertKind, Pet};
use crate::moment;

/// Energy at or below which the pet puts itself to bed.
pub const SLEEP_AT: u8 = 3;
/// Energy at which it has had enough sleep and gets up.
pub const WAKE_AT: u8 = 85;
/// Awake, this energetic and this bored means it will find its own fun.
pub const AMUSE_ENERGY: u8 = 70;
pub const AMUSE_BOREDOM: u8 = 45;
pub const AMUSE_GAIN: u8 = 4;
pub const AMUSE_COST: u8 = 5;
/// Pet-hours between bouts of self-amusement, so the 5 s tick cannot spam it.
pub const AMUSE_GAP_H: f64 = 1.0;

/// An event worth telling the player about, with its own wording.
pub type Event = (AlertKind, String);

// Stats are f64; gains and costs stay integer constants, converted at the call.
fn add(v: f64, d: u8) -> f64 {
    (v + f64::from(d)).clamp(0.0, 100.0)
}

fn sub(v: f64, d: u8) -> f64 {
    (v - f64::from(d)).clamp(0.0, 100.0)
}

fn schedule_next(pet: &mut Pet, now: u64, cfg: &Config, seed: u32) {
    pet.next_moment_seed = seed;
    pet.next_moment_ms = now.saturating_add(moment::next_gap_ms(seed, cfg.scale));
}

/// Advance the pet's own behaviour. Call after decay, with fresh entropy.
pub fn update(pet: &mut Pet, now: u64, cfg: &Config, fresh_seed: u32) -> Vec<Event> {
    let mut events = Vec::new();

    // A pet that has never been scheduled gets its first deadline now. Without
    // this a restored v1 save would sit at zero forever and fire immediately.
    if pet.next_moment_ms == 0 {
        schedule_next(pet, now, cfg, fresh_seed);
    }

    // 1. Expire whatever is running.
    if let Some(active) = pet.moment.clone() {
        if now >= active.ends_at_ms {
            pet.moment = None;
            schedule_next(pet, now, cfg, fresh_seed);
        }
    }

    // 2. Fire a new one if its hidden deadline has passed. The seed was drawn
    //    when the deadline was set, so the choice predates the moment arriving.
    if pet.moment.is_none() && now >= pet.next_moment_ms {
        let seed = pet.next_moment_seed;
        let set = moment::eligible(pet.fullness, pet.happiness, pet.energy, pet.cleanliness);
        if let Some(idx) = moment::pick(&set, seed) {
            if let Some(def) = moment::MOMENTS.get(idx as usize) {
                let ends = now.saturating_add(moment::duration_ms(idx, seed, cfg.scale));
                pet.moment = Some(moment::Active { idx, ends_at_ms: ends });
                events.push((AlertKind::Moment, format!("{} {}", pet.name, def.label)));
            }
        }
        // Schedule the next one whether or not anything was eligible, so a pet
        // in an unlucky state does not retry on every 5 s tick.
        schedule_next(pet, now, cfg, fresh_seed);
    }

    // 3. Look after itself.
    if !pet.sleeping && pet.energy <= f64::from(SLEEP_AT) {
        pet.sleeping = true;
        events.push((AlertKind::DozedOff, AlertKind::DozedOff.message(&pet.name)));
    } else if pet.sleeping && pet.energy >= f64::from(WAKE_AT) {
        pet.sleeping = false;
        events.push((AlertKind::WokeUp, AlertKind::WokeUp.message(&pet.name)));
    }

    // 4. Bored with energy to burn? Find something to do. Not during a moment —
    //    a moment already *is* the pet doing something.
    if !pet.sleeping
        && pet.moment.is_none()
        && pet.energy >= f64::from(AMUSE_ENERGY)
        && pet.happiness < f64::from(AMUSE_BOREDOM)
    {
        let gap_h = (now.saturating_sub(pet.last_amused_ms) as f64 / MS_PER_HOUR) * cfg.scale;
        if pet.last_amused_ms == 0 || gap_h >= AMUSE_GAP_H {
            pet.happiness = add(pet.happiness, AMUSE_GAIN);
            pet.energy = sub(pet.energy, AMUSE_COST);
            pet.last_amused_ms = now;
            events.push((
                AlertKind::AmusedItself,
                AlertKind::AmusedItself.message(&pet.name),
            ));
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;

    fn pet() -> Pet {
        Pet::new("Rex".into(), 0)
    }

    #[test]
    fn a_fresh_pet_gets_a_deadline_instead_of_firing_at_once() {
        let cfg = Config::default();
        let mut p = pet();
        let events = update(&mut p, 1_000, &cfg, 12345);
        assert!(p.next_moment_ms > 1_000, "a deadline must be scheduled");
        assert!(p.moment.is_none(), "nothing should fire on the first call");
        assert!(events.is_empty());
    }

    #[test]
    fn a_moment_fires_only_once_its_hidden_deadline_passes() {
        let cfg = Config::default();
        let mut p = pet();
        update(&mut p, 0, &cfg, 777);
        let deadline = p.next_moment_ms;

        update(&mut p, deadline - 1, &cfg, 778);
        assert!(p.moment.is_none(), "not due yet");

        let events = update(&mut p, deadline, &cfg, 779);
        assert!(p.moment.is_some(), "due now");
        assert!(events.iter().any(|(k, _)| *k == AlertKind::Moment));
    }

    #[test]
    fn spamming_the_tick_cannot_summon_a_moment() {
        let cfg = Config::default();
        let mut p = pet();
        update(&mut p, 0, &cfg, 999);
        // Hammer it far more often than the 5s tick ever would.
        for t in 1..2000u64 {
            update(&mut p, t * 10, &cfg, 1000 + t as u32);
        }
        assert!(
            p.moment.is_none(),
            "20s of frantic polling must not produce a moment"
        );
    }

    #[test]
    fn a_new_deadline_is_set_after_one_expires() {
        let cfg = Config::default();
        let mut p = pet();
        update(&mut p, 0, &cfg, 4242);
        let t = p.next_moment_ms;
        update(&mut p, t, &cfg, 4243);
        let active = p.moment.clone().expect("fired");

        update(&mut p, active.ends_at_ms, &cfg, 4244);
        assert!(p.moment.is_none(), "expired");
        assert!(p.next_moment_ms > active.ends_at_ms, "next one scheduled");
    }

    #[test]
    fn an_exhausted_pet_puts_itself_to_bed() {
        let cfg = Config::default();
        let mut p = pet();
        p.energy = 0.0;
        let events = update(&mut p, HOUR, &cfg, 1);
        assert!(p.sleeping, "it should not wait to be told");
        assert!(events.iter().any(|(k, _)| *k == AlertKind::DozedOff));
    }

    #[test]
    fn a_rested_pet_gets_itself_up_again() {
        let cfg = Config::default();
        let mut p = pet();
        p.sleeping = true;
        p.energy = 95.0;
        let events = update(&mut p, HOUR, &cfg, 1);
        assert!(!p.sleeping, "otherwise it would sleep forever");
        assert!(events.iter().any(|(k, _)| *k == AlertKind::WokeUp));
    }

    #[test]
    fn a_bored_energetic_pet_entertains_itself() {
        let cfg = Config::default();
        let mut p = pet();
        p.energy = 90.0;
        p.happiness = 20.0;
        let before = p.happiness;
        let events = update(&mut p, HOUR, &cfg, 1);
        assert!(p.happiness > before, "it found something to do");
        assert!(p.energy < 90.0, "and it cost energy");
        assert!(events.iter().any(|(k, _)| *k == AlertKind::AmusedItself));
    }

    #[test]
    fn self_amusement_cannot_be_farmed_by_the_tick() {
        let cfg = Config::default();
        let mut p = pet();
        p.energy = 100.0;
        p.happiness = 10.0;
        let mut bouts = 0;
        for t in 1..200u64 {
            let events = update(&mut p, t * 5_000, &cfg, t as u32);
            bouts += events
                .iter()
                .filter(|(k, _)| *k == AlertKind::AmusedItself)
                .count();
        }
        assert!(bouts <= 1, "16 minutes of ticks produced {bouts} bouts");
    }

    #[test]
    fn a_contented_pet_does_not_need_to_amuse_itself() {
        let cfg = Config::default();
        let mut p = pet();
        p.energy = 90.0;
        p.happiness = 90.0;
        let events = update(&mut p, HOUR, &cfg, 1);
        assert!(!events.iter().any(|(k, _)| *k == AlertKind::AmusedItself));
    }

    #[test]
    fn the_collection_counts_distinct_moments_not_sightings() {
        let mut p = pet();
        p.witness("zoomies");
        p.witness("zoomies");
        p.witness("sunbeam");
        assert_eq!(p.seen_moments.len(), 2);
    }
}
