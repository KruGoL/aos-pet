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

// No rounding here, ever: the watchdog invokes `advance` every 5 seconds, and
// at real-time scale a 5 s span decays a stat by ~0.01 points. Rounding that
// back onto an integer while `last_seen_ms` still advanced meant the pet never
// decayed at all. Fractions are kept; only the boundary (views, bars) rounds.
fn drop_stat(stat: f64, per_hour: f64, hours: f64) -> f64 {
    let delta = per_hour * hours;
    if !delta.is_finite() || delta <= 0.0 {
        return stat;
    }
    // Clamp: a very long absence must bottom out at 0, not go negative.
    (stat - delta).clamp(0.0, 100.0)
}

fn raise_stat(stat: f64, per_hour: f64, hours: f64) -> f64 {
    let delta = per_hour * hours;
    if !delta.is_finite() || delta <= 0.0 {
        return stat;
    }
    (stat + delta).clamp(0.0, 100.0)
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
    // Physical illness makes a pet miserable faster — but applying the same
    // multiplier to Gloom would mean sadness accelerates sadness, a spiral that
    // outruns the very cure it is supposed to motivate. Gloom is excluded so
    // cheering the pet up can actually gain ground.
    let physically_ill = crate::ailment::active(pet)
        .iter()
        .any(|a| *a != crate::ailment::Ailment::Gloom);
    let sad_mult = if physically_ill { cfg.sick_decay_factor } else { 1.0 };
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

    // Each ailment keeps its own clock, so the cure can be specific to the
    // cause rather than one universal "sick" flag.
    crate::ailment::accrue(pet, elapsed, cfg);
    pet.sick = crate::ailment::is_ill(pet);
    // Kept in step for the legacy field; nothing reads it for diagnosis now.
    pet.neglect_ms = pet.famine_ms.max(pet.grime_ms);

    pet.last_seen_ms = now_ms;

    let low = f64::from(LOW);
    let mut crossed = Vec::new();
    if was.0 >= low && pet.fullness < low {
        crossed.push(AlertKind::Hungry);
    }
    if was.1 >= low && pet.happiness < low {
        crossed.push(AlertKind::Sad);
    }
    if was.2 >= low && pet.energy < low {
        crossed.push(AlertKind::Tired);
    }
    if was.3 >= low && pet.cleanliness < low {
        crossed.push(AlertKind::Dirty);
    }
    if !was.4 && pet.sick {
        crossed.push(AlertKind::Sick);
    }
    // The symmetric edge. Gloom unwinds on its own once the pet cheers up, so
    // illness can end without anyone doing anything — without this the player
    // is told about every relapse and never about the recovery.
    if was.4 && !pet.sick {
        crossed.push(AlertKind::Recovered);
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
        assert_eq!(p.fullness, 80.0 - 8.0 * 5.0);
        assert_eq!(p.energy, 80.0 - 5.0 * 5.0);
        assert_eq!(p.last_seen_ms, 5 * HOUR);
    }

    #[test]
    fn a_very_long_absence_clamps_at_zero_and_never_wraps() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 10_000 * HOUR, &cfg);
        assert_eq!(p.fullness, 0.0);
        assert_eq!(p.happiness, 0.0);
        assert_eq!(p.energy, 0.0);
        assert_eq!(p.cleanliness, 0.0);
    }

    #[test]
    fn sleeping_restores_energy_and_slows_other_decay() {
        let cfg = Config::default();
        let mut awake = pet_at(0);
        let mut asleep = pet_at(0);
        asleep.sleeping = true;
        asleep.energy = 20.0;
        awake.energy = 20.0;

        advance(&mut awake, 2 * HOUR, &cfg);
        advance(&mut asleep, 2 * HOUR, &cfg);

        assert!(asleep.energy > awake.energy, "sleep must restore energy");
        assert!(asleep.energy > 20.0, "energy should rise while asleep");
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
        assert_eq!(p.fullness, 80.0, "no decay should be applied backwards");
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
        assert_eq!(p.fullness, 0.0);
        assert!(p.sick, "prolonged starvation must cause illness");
        assert!(alerts.contains(&AlertKind::Sick));

        let again = advance(&mut p, 40 * HOUR, &cfg);
        assert!(!again.contains(&AlertKind::Sick), "sickness alerts once");
    }

    #[test]
    fn recovery_stops_neglect_accruing_but_does_not_erase_it() {
        // The clock no longer snaps back to zero the instant a bar is full:
        // that is what `ailment::apply_remedy` is for, and it is why an illness
        // now takes sustained care to shake rather than one good meal.
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 15 * HOUR, &cfg);
        let accrued = p.famine_ms;
        assert!(accrued > 0);

        p.fullness = 100.0;
        p.cleanliness = 100.0;
        advance(&mut p, 16 * HOUR, &cfg);
        assert_eq!(p.famine_ms, accrued, "a full bar stops the clock, no more");
    }

    #[test]
    fn recovering_on_its_own_is_announced_just_like_falling_ill() {
        // Gloom unwinds without anyone lifting a finger, so recovery has to be
        // reported from here. Without the symmetric edge the player heard about
        // every relapse and never once about getting better.
        let cfg = Config::default();
        let mut p = pet_at(0);
        p.fullness = 100.0;
        p.cleanliness = 100.0;
        p.happiness = 0.0;
        let fell = advance(&mut p, 10 * HOUR, &cfg);
        assert!(fell.contains(&AlertKind::Sick), "got {fell:?}");

        // Cheer it up — and keep the other needs met, or the pet simply trades
        // gloom for famine and stays ill for an unrelated reason.
        p.happiness = 100.0;
        p.fullness = 100.0;
        p.cleanliness = 100.0;
        let rose = advance(&mut p, 20 * HOUR, &cfg);
        assert!(!p.sick, "gloom should have lifted, got {:?}", crate::ailment::active(&p));
        assert!(rose.contains(&AlertKind::Recovered), "got {rose:?}");
    }

    #[test]
    fn scale_compresses_time_for_demos() {
        let fast = Config::default().with_scale_str("60");
        let mut p = pet_at(0);
        advance(&mut p, 60_000, &fast); // one minute at 60x == one hour
        // (1/60)*60 is not exactly 1.0 in binary, so allow a hair of slack.
        assert!((p.fullness - 72.0).abs() < 1e-9, "got {}", p.fullness);
    }

    #[test]
    fn five_second_polling_decays_exactly_like_one_big_step() {
        // The watchdog invokes the capsule every 5 seconds. With integer stats
        // each 5 s span decayed a fraction of a point, rounded back to the same
        // integer — while `last_seen_ms` still advanced — so at real-time scale
        // the pet never decayed at all. This loop is TEST-ONLY: production stays
        // single-shot; it exists to prove many tiny spans sum like one big one.
        let cfg = Config::default();
        let mut polled = pet_at(0);
        for i in 1..=720u64 {
            advance(&mut polled, i * 5_000, &cfg); // 720 x 5 s = 1 hour
        }
        let mut single = pet_at(0);
        advance(&mut single, 3_600_000, &cfg);

        assert!(
            (single.fullness - 72.0).abs() < 1e-9,
            "one hour must cost 8 fullness, got {}",
            single.fullness
        );
        assert!(
            (polled.fullness - single.fullness).abs() < 0.5,
            "polling decayed to {} but a single step to {}",
            polled.fullness,
            single.fullness
        );
    }

    #[test]
    fn physical_illness_makes_happiness_fall_faster() {
        let cfg = Config::default();
        let mut well = pet_at(0);
        let mut ill = pet_at(0);
        ill.famine_ms = 99 * HOUR; // genuinely, physically unwell
        ill.sick = true;

        advance(&mut well, 2 * HOUR, &cfg);
        advance(&mut ill, 2 * HOUR, &cfg);
        assert!(ill.happiness < well.happiness);
    }

    #[test]
    fn gloom_alone_does_not_accelerate_its_own_decay() {
        // The anti-spiral guard: if gloom doubled happiness decay, being sad
        // would make the pet sadder faster than any amount of company could fix.
        let cfg = Config::default();
        let mut fine = pet_at(0);
        let mut gloomy = pet_at(0);
        gloomy.gloom_ms = 99 * HOUR;
        gloomy.sick = true;

        advance(&mut fine, 2 * HOUR, &cfg);
        advance(&mut gloomy, 2 * HOUR, &cfg);
        assert_eq!(
            gloomy.happiness, fine.happiness,
            "gloom must not compound itself"
        );
    }
}
