//! Anti-grind economy: never block an action, just stop paying for repetition.
//!
//! Blocking produces "please wait 34 seconds" tedium, and it fights a design
//! value — a starving pet must always be feedable, however badly it was
//! neglected. So the *need* an action serves always moves in full, while its
//! *happiness* payoff recharges over time. Spamming is allowed and simply worth
//! a fraction of spacing the same actions out.
//!
//! Deliberately expressed as arithmetic over a single timestamp rather than an
//! accumulator: `decay::advance` is single-shot, so anything that has to be
//! counted per elapsed period would either be wrong after a long absence or
//! require iterating once per period — a pet left for two years would spin the
//! 5 s watchdog thousands of times.

use crate::config::MS_PER_HOUR;

/// At or above this a need counts as fully served; serving it again is fussing.
pub const SATED: u8 = 90;
/// Happiness lost for pressing food on a pet that does not want it.
pub const OVERSERVE_PENALTY: u8 = 3;

/// How much of an action's happiness payoff has recharged, in `0.0..=1.0`.
///
/// Measured in pet-time, so a compressed `decay_scale` speeds recharge up in
/// step with decay — otherwise a demo pet would starve in minutes while its
/// rewards stayed on a four-hour real-world timer.
#[must_use]
pub fn readiness(last_ms: u64, now_ms: u64, ideal_hours: f64, scale: f64) -> f64 {
    if last_ms == 0 {
        return 1.0; // never done before — no reason to withhold the first payoff
    }
    if ideal_hours <= 0.0 || !ideal_hours.is_finite() {
        return 1.0;
    }
    // Clock skew must not hand out a bonus, so a negative gap reads as zero.
    let elapsed = now_ms.saturating_sub(last_ms) as f64;
    let hours = (elapsed / MS_PER_HOUR) * scale;
    (hours / ideal_hours).clamp(0.0, 1.0)
}

/// Scale a happiness payoff by readiness, rounding so a recharged action never
/// silently pays zero.
#[must_use]
pub fn payoff(base: u8, readiness: f64) -> u8 {
    if base == 0 {
        return 0;
    }
    let scaled = f64::from(base) * readiness.clamp(0.0, 1.0);
    let rounded = scaled.round() as u8;
    if readiness > 0.0 && rounded == 0 { 1 } else { rounded }
}

/// Words for why a payoff was small. "I fed it and nothing happened" is the
/// single most likely way this design reads as a bug, so every shrunken reward
/// has to explain itself.
#[must_use]
pub fn payoff_note(readiness: f64) -> &'static str {
    if readiness >= 0.95 {
        ""
    } else if readiness >= 0.5 {
        " It enjoyed that, though not as much as if you had waited a little longer."
    } else {
        " You only just did this, so it barely registered."
    }
}

/// True when serving this need again is fussing rather than care.
#[must_use]
pub fn is_overserving(current: f64) -> bool {
    current >= f64::from(SATED)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;

    #[test]
    fn a_first_time_action_pays_in_full() {
        assert_eq!(readiness(0, 5 * HOUR, 4.0, 1.0), 1.0);
    }

    #[test]
    fn readiness_recharges_linearly_and_caps() {
        assert!((readiness(0 + 1, 1 + 2 * HOUR, 4.0, 1.0) - 0.5).abs() < 0.01);
        assert_eq!(readiness(1, 1 + 4 * HOUR, 4.0, 1.0), 1.0);
        assert_eq!(readiness(1, 1 + 40 * HOUR, 4.0, 1.0), 1.0, "must not exceed 1");
    }

    #[test]
    fn spamming_is_allowed_but_nearly_worthless() {
        // Ten seconds after the last feed.
        let r = readiness(1, 1 + 10_000, 4.0, 1.0);
        assert!(r < 0.01, "got {r}");
        assert_eq!(payoff(25, r), 1, "still pays a token, never a flat zero");
    }

    #[test]
    fn spacing_actions_out_is_worth_several_times_more() {
        let spam = payoff(25, readiness(1, 1 + 60_000, 3.0, 1.0));
        let spaced = payoff(25, readiness(1, 1 + 3 * HOUR, 3.0, 1.0));
        assert!(
            spaced >= spam * 3,
            "spacing should dominate: spam={spam} spaced={spaced}"
        );
    }

    #[test]
    fn a_clock_moving_backwards_grants_no_bonus() {
        assert_eq!(readiness(10 * HOUR, 1 * HOUR, 4.0, 1.0), 0.0);
    }

    #[test]
    fn compressed_time_recharges_rewards_in_step_with_decay() {
        // One real minute at 60x is one pet-hour.
        let r = readiness(1, 1 + 60_000, 4.0, 60.0);
        assert!((r - 0.25).abs() < 0.01, "got {r}");
    }

    #[test]
    fn payoff_is_bounded_by_the_base() {
        assert_eq!(payoff(25, 1.0), 25);
        assert_eq!(payoff(25, 5.0), 25, "readiness above 1 must not overpay");
        assert_eq!(payoff(0, 1.0), 0);
    }

    #[test]
    fn notes_explain_a_shrunken_reward_and_stay_quiet_when_full() {
        assert_eq!(payoff_note(1.0), "");
        assert!(payoff_note(0.6).contains("waited"));
        assert!(payoff_note(0.01).contains("just did this"));
    }

    #[test]
    fn overserving_starts_at_the_sated_mark() {
        assert!(!is_overserving(f64::from(SATED) - 1.0));
        assert!(!is_overserving(f64::from(SATED) - 0.001));
        assert!(is_overserving(f64::from(SATED)));
        assert!(is_overserving(100.0));
    }
}
