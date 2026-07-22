//! The heart of the game: time-based decay.
//!
//! The capsule only runs when invoked, so nothing ticks in the background.
//! Instead every entry point calls [`advance`], which measures how much real
//! wall-clock time has passed since `last_seen_ms` and applies it in one shot.
//! The pet therefore keeps living while the daemon is stopped or the machine
//! is asleep — you cannot pause it by shutting AOS down.
//!
//! Pure arithmetic, no SDK imports: fully testable on the host.

use crate::config::{Config, LOW, MS_PER_HOUR};
use crate::model::{AlertKind, Pet};

fn drop_stat(stat: u8, per_hour: f64, hours: f64) -> u8 {
    let delta = per_hour * hours;
    if !delta.is_finite() || delta <= 0.0 {
        return stat;
    }
    // Saturate through f64 first: a very long absence must clamp to 0, not wrap.
    let next = f64::from(stat) - delta;
    if next <= 0.0 { 0 } else { next.round() as u8 }
}

fn raise_stat(stat: u8, per_hour: f64, hours: f64) -> u8 {
    let delta = per_hour * hours;
    if !delta.is_finite() || delta <= 0.0 {
        return stat;
    }
    let next = f64::from(stat) + delta;
    if next >= 100.0 { 100 } else { next.round() as u8 }
}

/// Advance the pet to `now_ms`, returning any thresholds newly crossed
/// (used to raise alerts). Idempotent for a repeated timestamp.
pub fn advance(pet: &mut Pet, now_ms: u64, cfg: &Config) -> Vec<AlertKind> {
    let elapsed = now_ms.saturating_sub(pet.last_seen_ms);
    if elapsed == 0 {
        // Clock went backwards or no time passed — still resync the marker.
        pet.last_seen_ms = now_ms;
        return Vec::new();
    }

    let hours = (elapsed as f64 / MS_PER_HOUR) * cfg.scale;
    let was = (
        pet.fullness,
        pet.happiness,
        pet.energy,
        pet.cleanliness,
        pet.sick,
    );

    let slow = if pet.sleeping { cfg.sleep_decay_factor } else { 1.0 };
    let sad_mult = if pet.sick { cfg.sick_decay_factor } else { 1.0 };
    // A moment colours the whole span it covers: dozing in a sunbeam is
    // restful, tearing around the room is not.
    let slow = slow * crate::behaviour::decay_multiplier(pet);

    pet.fullness = drop_stat(pet.fullness, cfg.fullness_per_hour * slow, hours);
    // Happiness deliberately ignores `slow`. If sleep paused everything it would
    // be a strictly dominant move — cheap to enter, cheap to leave, decay almost
    // halted — and the optimal way to play would be to keep the pet in a coma.
    // Making it trade mood for energy turns sleep into an actual decision.
    pet.happiness = drop_stat(pet.happiness, cfg.happiness_per_hour * sad_mult, hours);
    pet.cleanliness = drop_stat(pet.cleanliness, cfg.cleanliness_per_hour * slow, hours);
    pet.energy = if pet.sleeping {
        raise_stat(pet.energy, cfg.energy_recovery_per_hour, hours)
    } else {
        drop_stat(pet.energy, cfg.energy_per_hour, hours)
    };

    // Neglect only accrues while something is bottomed out. Any recovery resets it.
    if pet.fullness == 0 || pet.cleanliness == 0 {
        pet.neglect_ms = pet.neglect_ms.saturating_add(elapsed);
    } else {
        pet.neglect_ms = 0;
    }
    if pet.neglect_ms >= cfg.sick_after_ms {
        pet.sick = true;
    }

    pet.last_seen_ms = now_ms;

    let mut crossed = Vec::new();
    if was.0 >= LOW && pet.fullness < LOW {
        crossed.push(AlertKind::Hungry);
    }
    if was.1 >= LOW && pet.happiness < LOW {
        crossed.push(AlertKind::Sad);
    }
    if was.2 >= LOW && pet.energy < LOW {
        crossed.push(AlertKind::Tired);
    }
    if was.3 >= LOW && pet.cleanliness < LOW {
        crossed.push(AlertKind::Dirty);
    }
    if !was.4 && pet.sick {
        crossed.push(AlertKind::Sick);
    }
    crossed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Pet;

    const HOUR: u64 = 3_600_000;

    fn pet_at(now: u64) -> Pet {
        Pet::new("Rex".into(), now)
    }

    #[test]
    fn no_elapsed_time_changes_nothing() {
        let cfg = Config::default();
        let mut p = pet_at(1_000);
        let before = p.clone();
        let alerts = advance(&mut p, 1_000, &cfg);
        assert_eq!(p, before);
        assert!(alerts.is_empty());
    }

    #[test]
    fn stats_fall_over_time() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 5 * HOUR, &cfg);
        assert_eq!(p.fullness, 80 - (8.0 * 5.0) as u8);
        assert_eq!(p.energy, 80 - (5.0 * 5.0) as u8);
        assert_eq!(p.last_seen_ms, 5 * HOUR);
    }

    #[test]
    fn a_very_long_absence_clamps_at_zero_and_never_wraps() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 10_000 * HOUR, &cfg);
        assert_eq!(p.fullness, 0);
        assert_eq!(p.happiness, 0);
        assert_eq!(p.energy, 0);
        assert_eq!(p.cleanliness, 0);
    }

    #[test]
    fn sleeping_restores_energy_and_slows_other_decay() {
        let cfg = Config::default();
        let mut awake = pet_at(0);
        let mut asleep = pet_at(0);
        asleep.sleeping = true;
        asleep.energy = 20;
        awake.energy = 20;

        advance(&mut awake, 2 * HOUR, &cfg);
        advance(&mut asleep, 2 * HOUR, &cfg);

        assert!(asleep.energy > awake.energy, "sleep must restore energy");
        assert!(asleep.energy > 20, "energy should rise while asleep");
        assert!(
            asleep.fullness > awake.fullness,
            "other stats decay slower while asleep"
        );
    }

    #[test]
    fn sleep_does_not_shelter_happiness() {
        // Regression guard for the coma strategy: if sleeping slowed every stat,
        // parking the pet asleep would dominate actually looking after it.
        let cfg = Config::default();
        let mut awake = pet_at(0);
        let mut asleep = pet_at(0);
        asleep.sleeping = true;

        advance(&mut awake, 6 * HOUR, &cfg);
        advance(&mut asleep, 6 * HOUR, &cfg);

        assert_eq!(
            asleep.happiness, awake.happiness,
            "a sleeping pet must get just as lonely as a waking one"
        );
        assert!(asleep.energy > awake.energy, "sleep still pays in energy");
    }

    #[test]
    fn clock_moving_backwards_is_survived() {
        let cfg = Config::default();
        let mut p = pet_at(10 * HOUR);
        let alerts = advance(&mut p, 1 * HOUR, &cfg);
        assert!(alerts.is_empty());
        assert_eq!(p.fullness, 80, "no decay should be applied backwards");
        assert_eq!(p.last_seen_ms, 1 * HOUR);
    }

    #[test]
    fn crossing_low_raises_each_alert_once() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        let first = advance(&mut p, 8 * HOUR, &cfg);
        assert!(first.contains(&AlertKind::Hungry), "got {first:?}");

        // Crossing is edge-triggered: staying low must not re-alert.
        let second = advance(&mut p, 9 * HOUR, &cfg);
        assert!(!second.contains(&AlertKind::Hungry), "got {second:?}");
    }

    #[test]
    fn sustained_neglect_causes_sickness_exactly_once() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        // Long enough to bottom out fullness and then sit there.
        let alerts = advance(&mut p, 30 * HOUR, &cfg);
        assert_eq!(p.fullness, 0);
        assert!(p.sick, "prolonged starvation must cause illness");
        assert!(alerts.contains(&AlertKind::Sick));

        let again = advance(&mut p, 40 * HOUR, &cfg);
        assert!(!again.contains(&AlertKind::Sick), "sickness alerts once");
    }

    #[test]
    fn feeding_resets_the_neglect_clock() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 15 * HOUR, &cfg);
        assert!(p.neglect_ms > 0);

        p.fullness = 100;
        p.cleanliness = 100;
        advance(&mut p, 16 * HOUR, &cfg);
        assert_eq!(p.neglect_ms, 0, "recovery must clear accrued neglect");
    }

    #[test]
    fn scale_compresses_time_for_demos() {
        let fast = Config::default().with_scale_str("60");
        let mut p = pet_at(0);
        advance(&mut p, 60_000, &fast); // one minute at 60x == one hour
        assert_eq!(p.fullness, 80 - 8);
    }

    #[test]
    fn sickness_makes_happiness_fall_faster() {
        let cfg = Config::default();
        let mut well = pet_at(0);
        let mut ill = pet_at(0);
        ill.sick = true;

        advance(&mut well, 2 * HOUR, &cfg);
        advance(&mut ill, 2 * HOUR, &cfg);
        assert!(ill.happiness < well.happiness);
    }
}
