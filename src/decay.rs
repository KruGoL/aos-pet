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

/// Pet-hours of `span_h` a linearly-falling stat spends at or below
/// `threshold`, given where it started and how fast it falls. Closed form —
/// this is what lets neglect accounting stay single-shot without judging the
/// whole span by its end state.
///
/// The rate is the span's average when a moment weights it, so the crossing
/// time is approximate within the moment's window — minutes of error against
/// thresholds measured in hours.
fn hours_below(before: f64, threshold: f64, rate: f64, span_h: f64) -> f64 {
    if before <= threshold {
        return span_h;
    }
    if rate <= 0.0 || !rate.is_finite() {
        return 0.0;
    }
    (span_h - (before - threshold) / rate).max(0.0)
}

fn pet_hours_to_ms(h: f64) -> u64 {
    // Round, never truncate: systematic 1 ms undershoot once left an ailment
    // clock a hair before onset, which burned a billing segment on a
    // microscopic crossing and silently dropped the rest of the span.
    (h * MS_PER_HOUR).round() as u64
}

/// Multiplier from an active moment, weighted by how much of the span the
/// moment actually covers. `ends_at_ms` caps it: a restful sunbeam that ended
/// five minutes into a two-week absence colours five minutes, not two weeks.
fn moment_multiplier(pet: &Pet, now_ms: u64, elapsed_ms: u64) -> f64 {
    let Some(active) = pet.moment.as_ref() else {
        return 1.0;
    };
    let Some(def) = active.def() else {
        return 1.0;
    };
    let overlap = active
        .ends_at_ms
        .min(now_ms)
        .saturating_sub(pet.last_seen_ms);
    let frac = (overlap as f64 / elapsed_ms as f64).clamp(0.0, 1.0);
    def.decay_mult * frac + (1.0 - frac)
}

/// When the pet, awake or asleep, will next cross its own sleep boundary,
/// in pet-hours from now. Infinity when it is heading away from it.
fn sleep_crossing_h(sleeping: bool, energy: f64, cfg: &Config) -> f64 {
    use crate::behaviour::{SLEEP_AT, WAKE_AT};
    if sleeping {
        if cfg.energy_recovery_per_hour <= 0.0 {
            return f64::INFINITY;
        }
        ((f64::from(WAKE_AT) - energy) / cfg.energy_recovery_per_hour).max(0.0)
    } else {
        if cfg.energy_per_hour <= 0.0 {
            return f64::INFINITY;
        }
        ((energy - f64::from(SLEEP_AT)) / cfg.energy_per_hour).max(0.0)
    }
}

/// When a physical ailment will set in, in pet-hours from now: the bar must
/// first empty, then sit empty until its clock reaches onset. Closed form.
fn physical_onset_h(bar: f64, rate: f64, clock_ms: u64) -> f64 {
    let onset_left_h = (crate::ailment::ONSET_H - clock_ms as f64 / MS_PER_HOUR).max(0.0);
    let to_empty_h = if bar <= 0.0 {
        0.0
    } else if rate > 0.0 && rate.is_finite() {
        bar / rate
    } else {
        return f64::INFINITY;
    };
    to_empty_h + onset_left_h
}

/// Advance the pet to `now_ms`, returning any thresholds newly crossed
/// (used to raise alerts). Idempotent for a repeated timestamp.
///
/// The span is billed in SEGMENTS: the rates in force change when the pet
/// dozes off or wakes (energy crossing its own thresholds) and when a physical
/// ailment sets in (famine/grime clock reaching onset). Each boundary is found
/// in closed form and each segment is billed with constant rates — at most a
/// handful of segments, never a per-period loop. Without this, whatever state
/// the pet happened to be in when the span STARTED coloured the entire span:
/// putting the pet to sleep before a fortnight's absence billed two weeks at
/// sleep rates, and illness diagnosis depended on how often anything polled.
///
/// Only the first sleep transition is modelled; the remainder of the span is
/// billed in the post-transition state. The doze cycle beyond that would need
/// per-cycle work, and its error is bounded by a single cycle.
pub fn advance(pet: &mut Pet, now_ms: u64, cfg: &Config) -> Vec<AlertKind> {
    let elapsed = now_ms.saturating_sub(pet.last_seen_ms);
    if elapsed == 0 {
        // Clock went backwards or no time passed — still resync the marker.
        pet.last_seen_ms = now_ms;
        return Vec::new();
    }

    let total_h = (elapsed as f64 / MS_PER_HOUR) * cfg.scale;
    let was = (
        pet.fullness,
        pet.happiness,
        pet.energy,
        pet.cleanliness,
        pet.sick,
    );

    // A moment colours only the part of the span it actually overlaps. Applying
    // it to the whole span let one sunbeam slow an entire fortnight of absence.
    // Span-averaged rather than per-segment: a moment window is minutes against
    // boundaries measured in hours.
    let m_mult = moment_multiplier(pet, now_ms, elapsed);
    // Expire a moment that ended inside the span. This must happen HERE, not
    // only in behaviour::update: the prompt-injection path runs advance alone,
    // and an expired moment left `Some` had the agent telling the player what
    // the pet is doing "right now" hours after it stopped.
    if pet.moment.as_ref().is_some_and(|a| now_ms >= a.ends_at_ms) {
        pet.moment = None;
    }

    let mut crossed = Vec::new();
    let mut remaining_h = total_h;
    let mut sleep_done = false;

    // Bill a segment of `seg_h` pet-hours in the pet's CURRENT state. The
    // closure owns all the rate composition so the boundary loop below cannot
    // drift out of step with the tail billing.
    let bill = |pet: &mut Pet, seg_h: f64| {
        if seg_h <= 0.0 {
            return;
        }
        let physically_ill = crate::ailment::active(pet)
            .iter()
            .any(|a| *a != crate::ailment::Ailment::Gloom);
        let slow = if pet.sleeping { cfg.sleep_decay_factor } else { 1.0 } * m_mult;
        // Physical illness makes a pet miserable faster — but applying the
        // multiplier to Gloom would mean sadness accelerates sadness, a spiral
        // outrunning its own cure. Happiness also deliberately ignores `slow`:
        // if sleep paused everything, a coma would be the dominant strategy.
        let full_rate = cfg.fullness_per_hour * slow;
        let clean_rate = cfg.cleanliness_per_hour * slow;
        let joy_rate =
            cfg.happiness_per_hour * if physically_ill { cfg.sick_decay_factor } else { 1.0 };

        // Threshold times within the segment, before the bars move.
        let famine_h = hours_below(pet.fullness, 0.0, full_rate, seg_h);
        let grime_h = hours_below(pet.cleanliness, 0.0, clean_rate, seg_h);
        let below_h = hours_below(pet.happiness, f64::from(LOW), joy_rate, seg_h);

        pet.fullness = drop_stat(pet.fullness, full_rate, seg_h);
        pet.happiness = drop_stat(pet.happiness, joy_rate, seg_h);
        pet.cleanliness = drop_stat(pet.cleanliness, clean_rate, seg_h);
        pet.energy = if pet.sleeping {
            raise_stat(pet.energy, cfg.energy_recovery_per_hour, seg_h)
        } else {
            drop_stat(pet.energy, cfg.energy_per_hour, seg_h)
        };

        crate::ailment::accrue(
            pet,
            &crate::ailment::NeglectSpans {
                famine_ms: pet_hours_to_ms(famine_h),
                grime_ms: pet_hours_to_ms(grime_h),
                // Gloom pauses while the pet is physically ill: it is
                // miserable BECAUSE it is sick, and diagnosing a second
                // illness on top would cascade famine into gloom. Decided
                // per segment, so the span before onset still counts.
                gloom_below_ms: if physically_ill { 0 } else { pet_hours_to_ms(below_h) },
                gloom_above_ms: pet_hours_to_ms((seg_h - below_h).max(0.0)),
            },
        );
    };

    // Up to two modelled boundaries (one sleep transition, one physical
    // onset), each found in closed form; then the tail is billed
    // UNCONDITIONALLY. The tail must never depend on the loop exhausting
    // cleanly: a rounding hair once left an ailment clock 1 ms short of
    // onset, the "crossing" consumed the last iteration on a microscopic
    // segment, and the rest of a 10,000-hour span silently vanished while
    // last_seen still advanced.
    for _ in 0..2 {
        if remaining_h <= 0.0 {
            break;
        }
        let physically_ill = crate::ailment::active(pet)
            .iter()
            .any(|a| *a != crate::ailment::Ailment::Gloom);
        let slow = if pet.sleeping { cfg.sleep_decay_factor } else { 1.0 } * m_mult;
        let full_rate = cfg.fullness_per_hour * slow;
        let clean_rate = cfg.cleanliness_per_hour * slow;

        let sleep_cross = if sleep_done {
            f64::INFINITY
        } else {
            sleep_crossing_h(pet.sleeping, pet.energy, cfg)
        };
        let phys_cross = if physically_ill {
            f64::INFINITY
        } else {
            physical_onset_h(pet.fullness, full_rate, pet.famine_ms)
                .min(physical_onset_h(pet.cleanliness, clean_rate, pet.grime_ms))
        };

        let next_cross = sleep_cross.min(phys_cross);
        if next_cross >= remaining_h {
            break; // no boundary inside the span — the tail handles the rest
        }

        bill(pet, next_cross);
        remaining_h -= next_cross;
        if sleep_cross <= phys_cross && !sleep_done {
            // The pet crossed its own sleep boundary mid-span; flip it here so
            // the rest of the span is billed in the state it was actually in.
            pet.sleeping = !pet.sleeping;
            sleep_done = true;
            crossed.push(if pet.sleeping {
                AlertKind::DozedOff
            } else {
                AlertKind::WokeUp
            });
        }
        // A physical crossing needs no flip, but float dust must not leave a
        // clock 1 ms shy of the onset it was just billed up to — a degenerate
        // zero-length crossing here would burn the loop and let the tail be
        // billed as healthy. Snap the clock that caused the crossing.
        if phys_cross < sleep_cross {
            let onset = (crate::ailment::ONSET_H * MS_PER_HOUR) as u64;
            if pet.fullness <= 0.0 && pet.famine_ms.abs_diff(onset) <= 2 {
                pet.famine_ms = onset;
            }
            if pet.cleanliness <= 0.0 && pet.grime_ms.abs_diff(onset) <= 2 {
                pet.grime_ms = onset;
            }
        }
    }

    bill(pet, remaining_h);

    pet.sick = crate::ailment::is_ill(pet);
    // Kept in step for the legacy field; nothing reads it for diagnosis now.
    pet.neglect_ms = pet.famine_ms.max(pet.grime_ms);

    pet.last_seen_ms = now_ms;

    let low = f64::from(LOW);
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
        let alerts = advance(&mut p, 10_000 * HOUR, &cfg);
        assert_eq!(p.fullness, 0.0);
        assert_eq!(p.happiness, 0.0);
        assert_eq!(p.cleanliness, 0.0);
        // Energy is the exception now: the pet dozed off when it ran out and
        // slept the absence away (only the first transition is modelled), so
        // it comes back rested rather than impossibly at zero forever.
        assert!(p.sleeping, "it put itself to bed at the crossing");
        assert!(alerts.contains(&AlertKind::DozedOff));
        assert_eq!(p.energy, 100.0, "and slept its energy back");
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
    fn illness_no_longer_depends_on_how_often_the_pet_is_observed() {
        // The audit's probe, kept as a regression guard: charging the whole
        // span against the end-of-span bar meant one 12-hour absence produced
        // [Famine, Gloom] while the same 12 hours polled hourly produced
        // nothing. The closed form must make the two indistinguishable.
        let cfg = Config::default();
        let mut quiet = pet_at(0);
        advance(&mut quiet, 12 * HOUR, &cfg);

        let mut watched = pet_at(0);
        for i in 1..=12u64 {
            advance(&mut watched, i * HOUR, &cfg);
        }
        assert_eq!(
            quiet.famine_ms, watched.famine_ms,
            "one shot vs hourly must charge identical famine"
        );
        assert_eq!(quiet.sick, watched.sick);
    }

    #[test]
    fn a_pet_is_not_diagnosed_the_moment_its_bowl_empties() {
        // Fullness 80 at 8/h reaches zero at t=10h. The old accounting billed
        // all ten hours as famine and pronounced the pet ill on arrival.
        let cfg = Config::default();
        let mut p = pet_at(0);
        advance(&mut p, 10 * HOUR, &cfg);
        assert_eq!(p.famine_ms, 0, "the decline is not neglect");
        assert!(!p.sick);

        // Six further hours AT zero is what earns the illness.
        advance(&mut p, 16 * HOUR, &cfg);
        assert!(p.sick, "got famine_ms={}", p.famine_ms);
    }

    #[test]
    fn compressed_time_makes_illness_arrive_sooner_in_real_seconds() {
        // Moved from ailment.rs when accrual went closed-form: scale is decay's
        // concern now. 40 real seconds at 2000x is ~22 pet-hours — ten to empty
        // the bowl, twelve at zero, comfortably past onset.
        let fast = Config::default().with_scale_str("2000");
        let mut p = pet_at(0);
        advance(&mut p, 40_000, &fast);
        assert!(p.sick, "demos must be possible, famine_ms={}", p.famine_ms);
    }

    #[test]
    fn a_moment_colours_only_the_time_it_actually_covers() {
        // A one-hour sunbeam inside a five-hour span slows one hour of decay,
        // not five. Before the weighting, a restful moment at the start of a
        // fortnight's absence discounted the entire fortnight.
        let cfg = Config::default();
        let calm = crate::moment::MOMENTS
            .iter()
            .position(|m| (m.decay_mult - 0.5).abs() < 1e-9)
            .expect("the roster has a 0.5x moment") as u16;

        let mut basked = pet_at(0);
        basked.moment = Some(crate::moment::Active { idx: calm, ends_at_ms: HOUR });
        advance(&mut basked, 5 * HOUR, &cfg);

        // frac = 1/5, so the effective multiplier is 0.5*0.2 + 0.8 = 0.9.
        let expected = 80.0 - 8.0 * 0.9 * 5.0;
        assert!(
            (basked.fullness - expected).abs() < 1e-9,
            "got {}, want {expected}",
            basked.fullness
        );

        let mut plain = pet_at(0);
        advance(&mut plain, 5 * HOUR, &cfg);
        assert!(basked.fullness > plain.fullness, "the sunbeam still helped");
    }

    #[test]
    fn gloom_pauses_from_the_instant_physical_illness_sets_in() {
        // The pause is decided per SEGMENT, at the onset crossing — not by the
        // end state of the whole span, which made diagnosis depend on how often
        // the pet was polled. A miserable-but-fed pet accrues gloom right up to
        // the moment famine sets in, and none after.
        let cfg = Config::default();
        let mut quiet = pet_at(0);
        quiet.happiness = 0.0;
        // Fullness 80 empties at 10h; famine onsets at 16h. 20h of span.
        advance(&mut quiet, 20 * HOUR, &cfg);

        let mut watched = pet_at(0);
        watched.happiness = 0.0;
        for i in 1..=20u64 {
            advance(&mut watched, i * HOUR, &cfg);
        }
        assert_eq!(
            quiet.gloom_ms, watched.gloom_ms,
            "one shot vs hourly must charge identical gloom"
        );
        assert_eq!(
            quiet.gloom_ms,
            16 * HOUR,
            "gloom runs until famine onset at t=16h and pauses there"
        );
    }

    #[test]
    fn a_sleeping_pet_does_not_bill_a_whole_absence_at_sleep_rates() {
        // The sleep discount used to be sampled once at span entry: put the pet
        // to sleep, leave for a fortnight, and fourteen days were billed at
        // 0.4x. The segmented form wakes it at the WAKE_AT crossing and bills
        // the remainder awake.
        let cfg = Config::default();
        let mut p = pet_at(0);
        p.sleeping = true;
        p.energy = 80.0; // wakes at (85-80)/20 = 0.25h
        let alerts = advance(&mut p, 40 * HOUR, &cfg);

        assert!(!p.sleeping, "it woke up mid-span");
        assert!(alerts.contains(&AlertKind::WokeUp), "and said so");
        // Awake, the bowl empties at ~10.15h and famine hits the 18h ceiling
        // (40 − 10.15 > 18). Billed asleep for the whole span it would have
        // emptied at 25h and reached only 15h — below the cap. The cap value
        // is therefore itself the proof the pet was billed awake.
        let cap = (crate::ailment::MAX_DEPTH_H * MS_PER_HOUR) as u64;
        assert_eq!(
            p.famine_ms, cap,
            "famine must accrue from the awake empty-time, got {}h",
            p.famine_ms / HOUR
        );
    }

    #[test]
    fn an_exhausted_pet_dozes_off_mid_span_and_the_bill_says_so() {
        let cfg = Config::default();
        let mut p = pet_at(0);
        p.energy = 8.0; // crosses SLEEP_AT=3 after 1h awake at 5/h
        let alerts = advance(&mut p, 3 * HOUR, &cfg);
        assert!(p.sleeping, "it put itself to bed inside the span");
        assert!(alerts.contains(&AlertKind::DozedOff));
        // 1h awake (8 -> 3, hitting SLEEP_AT exactly), then 2h asleep at 20/h.
        let expected = (3.0 + 2.0 * 20.0_f64).min(100.0);
        assert!(
            (p.energy - expected).abs() < 1e-6,
            "energy must be billed piecewise: got {}, want {expected}",
            p.energy
        );
    }

    #[test]
    fn an_expired_moment_is_cleared_by_advance_itself() {
        // The prompt-injection path runs advance alone — if only
        // behaviour::update expired moments, the agent kept announcing what the
        // pet is doing "right now" hours after it stopped.
        let cfg = Config::default();
        let mut p = pet_at(0);
        p.moment = Some(crate::moment::Active { idx: 0, ends_at_ms: 60_000 });
        advance(&mut p, 4 * HOUR, &cfg);
        assert!(p.moment.is_none(), "advance must expire it");
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

