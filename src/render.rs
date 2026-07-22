//! Turning pet state into something a human wants to look at.

use crate::art;
use crate::config::LOW;
use crate::model::Pet;
use crate::mood::{mood_of, Mood};

pub const BAR_WIDTH: usize = 12;

/// A 0..=100 stat as a fixed-width bar.
#[must_use]
pub fn bar(value: u8, width: usize) -> String {
    let v = usize::from(value.min(100));
    // Round to nearest cell so 100 always fills and 0 always empties.
    let filled = ((v * width) + 50) / 100;
    let filled = filled.min(width);
    let mut s = String::with_capacity(width);
    for _ in 0..filled {
        s.push('#');
    }
    for _ in filled..width {
        s.push('.');
    }
    s
}

fn stat_line(label: &str, value: u8) -> String {
    format!("  {label:<12}[{}] {value:>3}\n", bar(value, BAR_WIDTH))
}

/// Full human-facing view: art frame, name, mood, bars and status flags.
#[must_use]
pub fn display(pet: &Pet, frame: usize, now_ms: u64) -> String {
    let mood = mood_of(pet);
    let art_frame = art::frames(mood)[frame % art::FRAME_COUNT];
    let age_hours = pet.age_ms(now_ms) / 3_600_000;

    let mut out = String::new();
    out.push_str(art_frame);
    out.push_str("\n\n");
    out.push_str(&format!("  {} — {}\n", pet.name, mood.as_str()));
    out.push_str(&stat_line("Fullness", pet.fullness));
    out.push_str(&stat_line("Happiness", pet.happiness));
    out.push_str(&stat_line("Energy", pet.energy));
    out.push_str(&stat_line("Cleanliness", pet.cleanliness));
    out.push_str(&format!("  age {age_hours}h"));
    if pet.sick {
        out.push_str("   [ILL — try pet_heal]");
    }
    if pet.sleeping {
        out.push_str("   [asleep — pet_sleep wake=true]");
    }
    out.push('\n');
    out
}

/// Both animation frames, for a viewer that wants to animate locally.
#[must_use]
pub fn frames_for(pet: &Pet, now_ms: u64) -> Vec<String> {
    (0..art::FRAME_COUNT)
        .map(|i| display(pet, i, now_ms))
        .collect()
}

#[must_use]
pub fn mood_name(pet: &Pet) -> &'static str {
    mood_of(pet).as_str()
}

/// How worried a client should look, as a semantic level rather than a colour.
///
/// The capsule deliberately does not know about tmux, ANSI or any other client:
/// it reports severity, and each client maps that onto its own palette.
#[must_use]
pub fn level(pet: &Pet) -> &'static str {
    let bottomed = pet.fullness == 0
        || pet.happiness == 0
        || pet.energy == 0
        || pet.cleanliness == 0;
    if pet.sick || bottomed {
        return "critical";
    }
    if pet.sleeping {
        return "resting";
    }
    if pet.fullness < LOW || pet.happiness < LOW || pet.energy < LOW || pet.cleanliness < LOW {
        return "warn";
    }
    "ok"
}

/// A tiny one-line summary, sized for a status bar or shell prompt.
/// Lives here so every client renders the pet identically.
#[must_use]
pub fn compact(pet: &Pet) -> String {
    let face = match mood_of(pet) {
        Mood::Happy => "^.^",
        Mood::Neutral => "o.o",
        Mood::Hungry => "O.O",
        Mood::Tired => "-.-",
        Mood::Dirty => "x.o",
        Mood::Sad => ";.;",
        Mood::Sick => "x.x",
        Mood::Sleeping => "u.u",
    };
    let mut s = format!(
        "{} ({}) f{} h{} e{} c{}",
        pet.name, face, pet.fullness, pet.happiness, pet.energy, pet.cleanliness
    );
    if pet.sick {
        s.push_str(" [SICK]");
    }
    if pet.sleeping {
        s.push_str(" [zzz]");
    }
    s
}

/// A compact briefing folded into the agent's system prompt so it can raise
/// the pet's needs unprompted. Kept short — this rides in every single turn.
#[must_use]
pub fn prompt_section(pet: &Pet) -> String {
    let mut s = format!(
        "# Virtual pet\n\nThe user looks after a pet named {} (currently {}). \
         Fullness {}/100, happiness {}/100, energy {}/100, cleanliness {}/100.",
        pet.name,
        mood_name(pet),
        pet.fullness,
        pet.happiness,
        pet.energy,
        pet.cleanliness
    );
    if pet.sick {
        s.push_str(" It is ILL and needs pet_heal.");
    }
    if pet.sleeping {
        s.push_str(" It is asleep right now.");
    }
    s.push_str(
        " If it needs something, mention it naturally in your reply. \
         Tools: pet_status, pet_feed, pet_play, pet_sleep, pet_clean, pet_heal.",
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_endpoints_are_exact() {
        assert_eq!(bar(0, 10), "..........");
        assert_eq!(bar(100, 10), "##########");
    }

    #[test]
    fn bar_is_always_the_requested_width() {
        for v in 0..=100u8 {
            assert_eq!(bar(v, BAR_WIDTH).chars().count(), BAR_WIDTH, "value {v}");
        }
    }

    #[test]
    fn bar_clamps_impossible_values() {
        assert_eq!(bar(200, 10), "##########");
    }

    #[test]
    fn bar_is_monotonic() {
        let mut last = 0;
        for v in 0..=100u8 {
            let filled = bar(v, BAR_WIDTH).chars().filter(|c| *c == '#').count();
            assert!(filled >= last, "bar shrank at {v}");
            last = filled;
        }
    }

    #[test]
    fn display_shows_name_mood_and_every_stat() {
        let p = Pet::new("Rex".into(), 0);
        let out = display(&p, 0, 3_600_000);
        assert!(out.contains("Rex"));
        assert!(out.contains("Fullness"));
        assert!(out.contains("Happiness"));
        assert!(out.contains("Energy"));
        assert!(out.contains("Cleanliness"));
        assert!(out.contains("age 1h"));
    }

    #[test]
    fn display_flags_illness_and_sleep() {
        let mut p = Pet::new("Rex".into(), 0);
        p.sick = true;
        assert!(display(&p, 0, 0).contains("ILL"));

        let mut s = Pet::new("Rex".into(), 0);
        s.sleeping = true;
        assert!(display(&s, 0, 0).contains("asleep"));
    }

    #[test]
    fn level_escalates_with_the_pets_condition() {
        let mut p = Pet::new("Rex".into(), 0);
        p.fullness = 100;
        p.happiness = 100;
        p.energy = 100;
        p.cleanliness = 100;
        assert_eq!(level(&p), "ok");

        p.happiness = LOW - 1;
        assert_eq!(level(&p), "warn", "a low stat is a warning");

        p.happiness = 0;
        assert_eq!(level(&p), "critical", "a bottomed stat is critical");
    }

    #[test]
    fn illness_is_always_critical_even_with_perfect_stats() {
        let mut p = Pet::new("Rex".into(), 0);
        p.fullness = 100;
        p.happiness = 100;
        p.energy = 100;
        p.cleanliness = 100;
        p.sick = true;
        assert_eq!(level(&p), "critical");
    }

    #[test]
    fn sleeping_reads_as_resting_unless_something_is_actually_wrong() {
        let mut p = Pet::new("Rex".into(), 0);
        p.sleeping = true;
        assert_eq!(level(&p), "resting");

        // Asleep is no excuse for starving.
        p.fullness = 0;
        assert_eq!(level(&p), "critical");
    }

    #[test]
    fn compact_fits_a_status_bar_and_shows_flags() {
        let mut p = Pet::new("Rex".into(), 0);
        let line = compact(&p);
        assert!(line.starts_with("Rex"));
        assert!(line.contains("f80"));
        assert!(line.len() < 60, "status-bar line must stay short: {line}");
        assert!(!line.contains('\n'), "must be a single line");

        p.sick = true;
        assert!(compact(&p).contains("[SICK]"));
        p.sleeping = true;
        assert!(compact(&p).contains("[zzz]"));
    }

    #[test]
    fn compact_face_tracks_the_mood() {
        let mut p = Pet::new("Rex".into(), 0);
        p.fullness = 100;
        p.happiness = 100;
        p.energy = 100;
        p.cleanliness = 100;
        assert!(compact(&p).contains("^.^"), "thriving pet should smile");

        p.fullness = 5;
        assert!(compact(&p).contains("O.O"), "hungry pet should look alarmed");
    }

    #[test]
    fn prompt_section_states_the_facts_and_flags_illness() {
        let mut p = Pet::new("Rex".into(), 0);
        let normal = prompt_section(&p);
        assert!(normal.contains("Rex"));
        assert!(normal.contains("80/100"));
        assert!(normal.contains("pet_feed"));
        assert!(!normal.contains("ILL"));

        p.sick = true;
        assert!(prompt_section(&p).contains("ILL"));
    }

    #[test]
    fn prompt_section_stays_small_enough_to_ride_every_turn() {
        let p = Pet::new("Rex".into(), 0);
        assert!(prompt_section(&p).len() < 600, "prompt injection must stay compact");
    }

    #[test]
    fn both_frames_render_and_differ() {
        let p = Pet::new("Rex".into(), 0);
        let f = frames_for(&p, 0);
        assert_eq!(f.len(), art::FRAME_COUNT);
        assert_ne!(f[0], f[1]);
    }
}
