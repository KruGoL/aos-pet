//! Pet state. Pure data + serde — no SDK imports, so it is host-testable.

use serde::{Deserialize, Serialize};

/// KV key holding this principal's pet. KV is principal-scoped, so one key
/// per capsule yields one pet per user with no extra work.
pub const KV_KEY: &str = "pet";
pub const STATE_VERSION: u32 = 1;
pub const MAX_ALERTS: usize = 20;
pub const MAX_NAME: usize = 32;
/// Stats a freshly adopted pet starts with.
pub const START_STAT: f64 = 80.0;

fn default_version() -> u32 {
    STATE_VERSION
}

// Stats are f64 so sub-point decay accumulates honestly: the 5 s watchdog tick
// spans decay fractions of a point, and integer stats rounded them all away.
// No `Eq` — f64 is only `PartialEq`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pet {
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    pub born_at_ms: u64,
    pub last_seen_ms: u64,
    pub fullness: f64,
    pub happiness: f64,
    pub energy: f64,
    pub cleanliness: f64,
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
    /// When the pet last entertained itself, so the 5 s tick cannot farm it.
    #[serde(default)]
    pub last_amused_ms: u64,
    /// When medicine was last given. Without this, `pet_heal` is the one action
    /// with no readiness clock, and spamming it becomes a master key that cures
    /// everything — exactly what the three-ailment design exists to prevent.
    #[serde(default)]
    pub last_healed_ms: u64,
    #[serde(default)]
    pub alerts: Vec<Alert>,
    #[serde(default)]
    pub last_alert_ms: u64,

    // --- rare moments ---
    /// The moment currently happening, if any.
    #[serde(default)]
    pub moment: Option<crate::moment::Active>,
    /// Wall-clock deadline for the next moment, and the seed that already
    /// decided which one it will be. Never leaves the capsule — that is what
    /// makes moments impossible to summon or steer.
    #[serde(default)]
    pub next_moment_ms: u64,
    #[serde(default)]
    pub next_moment_seed: u32,
    /// Keys of moments the player has actually witnessed — the collection.
    #[serde(default)]
    pub seen_moments: Vec<String>,
    /// Friendly scraps won. Never decreases; losing costs only the energy spent.
    #[serde(default)]
    pub victories: u32,

    /// Neglect clocks, one per ailment. Each is `+= elapsed` once per span, so
    /// a long absence costs a single addition rather than an iteration.
    #[serde(default)]
    pub famine_ms: u64,
    #[serde(default)]
    pub grime_ms: u64,
    #[serde(default)]
    pub gloom_ms: u64,
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
            last_amused_ms: 0,
            last_healed_ms: 0,
            alerts: Vec::new(),
            last_alert_ms: 0,
            moment: None,
            next_moment_ms: 0,
            next_moment_seed: 0,
            seen_moments: Vec::new(),
            victories: 0,
            famine_ms: 0,
            grime_ms: 0,
            gloom_ms: 0,
        }
    }

    /// Record a moment as witnessed. Idempotent — the collection counts
    /// distinct moments, not sightings.
    pub fn witness(&mut self, key: &str) {
        if !self.seen_moments.iter().any(|k| k == key) {
            self.seen_moments.push(key.to_string());
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
        let message = kind.message(&self.name);
        self.push_alert_with(kind, at_ms, message);
    }

    /// Append an alert that carries its own wording — moments name themselves.
    pub fn push_alert_with(&mut self, kind: AlertKind, at_ms: u64, message: String) {
        self.alerts.push(Alert {
            at_ms,
            kind,
            message,
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
