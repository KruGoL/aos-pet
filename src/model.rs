//! Pet state. Pure data + serde — no SDK imports, so it is host-testable.

use serde::{Deserialize, Serialize};

/// KV key holding this principal's pet. KV is principal-scoped, so one key
/// per capsule yields one pet per user with no extra work.
pub const KV_KEY: &str = "pet";
pub const STATE_VERSION: u32 = 1;
pub const MAX_ALERTS: usize = 20;
pub const MAX_NAME: usize = 32;
/// Stats a freshly adopted pet starts with.
pub const START_STAT: u8 = 80;

fn default_version() -> u32 {
    STATE_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pet {
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub born_at_ms: u64,
    pub last_seen_ms: u64,
    pub fullness: u8,
    pub happiness: u8,
    pub energy: u8,
    pub cleanliness: u8,
    #[serde(default)]
    pub sleeping: bool,
    #[serde(default)]
    pub sick: bool,
    /// Accumulated time spent with a stat pinned at zero.
    #[serde(default)]
    pub neglect_ms: u64,
    /// When each kind of care last happened, for the readiness economy.
    /// Zero means "never", which pays in full — see `economy::readiness`.
    #[serde(default)]
    pub last_fed_ms: u64,
    #[serde(default)]
    pub last_played_ms: u64,
    #[serde(default)]
    pub last_cleaned_ms: u64,
    #[serde(default)]
    pub alerts: Vec<Alert>,
    #[serde(default)]
    pub last_alert_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub at_ms: u64,
    pub kind: AlertKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Hungry,
    Sad,
    Tired,
    Dirty,
    Sick,
    Recovered,
    /// The pet put itself to bed rather than waiting to be told.
    DozedOff,
    /// Slept itself out and got up again.
    WokeUp,
    /// Bored with energy to burn, so it found its own entertainment.
    AmusedItself,
    /// A rare moment began. The message carries which one.
    Moment,
}

impl AlertKind {
    #[must_use]
    pub fn message(self, name: &str) -> String {
        match self {
            Self::Hungry => format!("{name} is getting hungry."),
            Self::Sad => format!("{name} is feeling lonely — play together?"),
            Self::Tired => format!("{name} is running out of energy."),
            Self::Dirty => format!("{name} could use a wash."),
            Self::Sick => format!("{name} has fallen ill and needs healing."),
            Self::Recovered => format!("{name} is feeling healthy again!"),
            Self::DozedOff => format!("{name} curled up and fell asleep on its own."),
            Self::WokeUp => format!("{name} slept itself out and got up."),
            Self::AmusedItself => format!("{name} got bored and found something to do."),
            // Moments carry their own text; this is only the fallback.
            Self::Moment => format!("{name} is having a moment."),
        }
    }
}

impl Pet {
    #[must_use]
    pub fn new(name: String, now_ms: u64) -> Self {
        Self {
            version: STATE_VERSION,
            name,
            born_at_ms: now_ms,
            last_seen_ms: now_ms,
            fullness: START_STAT,
            happiness: START_STAT,
            energy: START_STAT,
            cleanliness: START_STAT,
            sleeping: false,
            sick: false,
            neglect_ms: 0,
            last_fed_ms: 0,
            last_played_ms: 0,
            last_cleaned_ms: 0,
            alerts: Vec::new(),
            last_alert_ms: 0,
        }
    }

    /// Trim and bound a requested name. Empty input falls back to a default so
    /// a pet always has something to be called.
    #[must_use]
    pub fn sanitize_name(raw: &str) -> String {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return "Blob".to_string();
        }
        trimmed.chars().take(MAX_NAME).collect()
    }

    #[must_use]
    pub fn age_ms(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.born_at_ms)
    }

    /// Append an alert, keeping only the most recent `MAX_ALERTS`.
    pub fn push_alert(&mut self, kind: AlertKind, at_ms: u64) {
        self.alerts.push(Alert {
            at_ms,
            kind,
            message: kind.message(&self.name),
        });
        if self.alerts.len() > MAX_ALERTS {
            let excess = self.alerts.len() - MAX_ALERTS;
            self.alerts.drain(0..excess);
        }
        self.last_alert_ms = at_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pet_starts_content_and_healthy() {
        let p = Pet::new("Rex".into(), 1_000);
        assert_eq!(p.fullness, START_STAT);
        assert_eq!(p.born_at_ms, 1_000);
        assert_eq!(p.last_seen_ms, 1_000);
        assert!(!p.sick);
        assert!(!p.sleeping);
        assert!(p.alerts.is_empty());
    }

    #[test]
    fn name_is_trimmed_bounded_and_never_empty() {
        assert_eq!(Pet::sanitize_name("  Rex  "), "Rex");
        assert_eq!(Pet::sanitize_name("   "), "Blob");
        assert_eq!(Pet::sanitize_name(&"x".repeat(100)).chars().count(), MAX_NAME);
    }

    #[test]
    fn alerts_are_capped_keeping_newest() {
        let mut p = Pet::new("Rex".into(), 0);
        for i in 0..(MAX_ALERTS as u64 + 5) {
            p.push_alert(AlertKind::Hungry, i);
        }
        assert_eq!(p.alerts.len(), MAX_ALERTS);
        assert_eq!(p.alerts.last().unwrap().at_ms, MAX_ALERTS as u64 + 4);
        assert_eq!(p.alerts.first().unwrap().at_ms, 5);
    }

    #[test]
    fn state_survives_a_json_round_trip() {
        let mut p = Pet::new("Rex".into(), 42);
        p.push_alert(AlertKind::Sick, 99);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Pet>(&json).unwrap(), p);
    }

    #[test]
    fn older_state_without_new_fields_still_loads() {
        // Fields added after v1 must have serde defaults, or existing pets die
        // on upgrade. This is the regression guard for that.
        let legacy = r#"{
            "name":"Rex","born_at_ms":1,"last_seen_ms":2,
            "fullness":50,"happiness":50,"energy":50,"cleanliness":50
        }"#;
        let p: Pet = serde_json::from_str(legacy).unwrap();
        assert_eq!(p.name, "Rex");
        assert_eq!(p.version, STATE_VERSION);
        assert!(!p.sick);
        assert!(p.alerts.is_empty());
    }
}
