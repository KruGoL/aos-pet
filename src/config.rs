//! Tunable game constants. Deliberately free of SDK imports so the rules
//! stay testable on the development host.

/// A stat at or below this is "in trouble" and triggers an alert crossing.
pub const LOW: u8 = 25;
/// Every stat at or above this means the pet is thriving.
pub const HIGH: u8 = 70;
/// Everything brimming — the pet is not merely fine, it is radiant.
pub const PEAK: u8 = 90;

pub const MS_PER_HOUR: f64 = 3_600_000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Config {
    /// Multiplies elapsed time. 1.0 = real time; 60.0 turns hours into minutes.
    pub scale: f64,
    pub fullness_per_hour: f64,
    pub happiness_per_hour: f64,
    pub energy_per_hour: f64,
    pub cleanliness_per_hour: f64,
    /// Energy regained per hour while asleep.
    pub energy_recovery_per_hour: f64,
    /// Decay is multiplied by this while the pet sleeps.
    pub sleep_decay_factor: f64,
    /// Happiness decay is multiplied by this while the pet is sick.
    pub sick_decay_factor: f64,
    /// Hours after which each kind of care pays its full happiness bonus again.
    /// Shorter gaps still work — they are simply worth proportionally less.
    pub feed_ideal_hours: f64,
    pub play_ideal_hours: f64,
    pub clean_ideal_hours: f64,
    pub heal_ideal_hours: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scale: 1.0,
            fullness_per_hour: 8.0,
            happiness_per_hour: 6.0,
            energy_per_hour: 5.0,
            cleanliness_per_hour: 4.0,
            energy_recovery_per_hour: 20.0,
            sleep_decay_factor: 0.4,
            sick_decay_factor: 2.0,
            feed_ideal_hours: 4.0,
            play_ideal_hours: 3.0,
            clean_ideal_hours: 12.0,
            // Longest of the four: medicine is meant to be occasional relief,
            // not part of the daily routine.
            heal_ideal_hours: 8.0,
        }
    }
}

impl Config {
    /// Apply a `decay_scale` string from capsule config. Anything unparseable,
    /// non-finite or non-positive leaves the default untouched — a bad config
    /// value must never freeze or explode the pet.
    #[must_use]
    pub fn with_scale_str(mut self, raw: &str) -> Self {
        if let Ok(parsed) = raw.trim().parse::<f64>() {
            if parsed.is_finite() && parsed > 0.0 {
                self.scale = parsed;
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_scale_is_applied() {
        assert_eq!(Config::default().with_scale_str("60").scale, 60.0);
        assert_eq!(Config::default().with_scale_str("  2.5 ").scale, 2.5);
    }

    #[test]
    fn bad_scale_falls_back_to_default() {
        let d = Config::default().scale;
        for bad in ["", "abc", "0", "-3", "NaN", "inf"] {
            assert_eq!(
                Config::default().with_scale_str(bad).scale,
                d,
                "input {bad:?} should not change the scale"
            );
        }
    }
}
