//! Turning pet state into something a human wants to look at.

use crate::art;
use crate::config::LOW;
use crate::model::Pet;
use crate::mood::{mood_of, Mood};

pub const BAR_WIDTH: usize = 12;

/// Round a fractional stat for the outside world. Stats carry fractions
/// internally so slow decay accumulates; players only ever see whole points —
/// "80.00000001" must never reach a screen.
#[must_use]
pub fn pt(v: f64) -> u8 {
    v.round().clamp(0.0, 100.0) as u8
}

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
    // A moment takes over the picture — that is what makes it noticeable at a
    // glance rather than a line of text nobody reads.
    let owned;
    let art_frame = match pet.moment.as_ref().and_then(crate::moment::Active::def) {
        Some(m) => {
            owned = art::compose(m.face[frame % art::FRAME_COUNT], m.fx);
            owned.as_str()
        }
        None => art::frames(mood)[frame % art::FRAME_COUNT],
    };
    let age_hours = pet.age_ms(now_ms) / 3_600_000;

    let mut out = String::new();
    out.push_str(art_frame);
    out.push_str("\n\n");
    out.push_str(&format!("  {} — {}\n", pet.name, mood.as_str()));
    // A moment is the most interesting thing that can be true right now, so it
    // goes above the bars rather than in a footnote.
    if let Some(label) = pet.moment.as_ref().and_then(crate::moment::Active::def) {
        out.push_str(&format!("  * {} {}\n", pet.name, label.label));
    }
    out.push_str(&stat_line("Fullness", pt(pet.fullness)));
    out.push_str(&stat_line("Happiness", pt(pet.happiness)));
    out.push_str(&stat_line("Energy", pt(pet.energy)));
    out.push_str(&stat_line("Cleanliness", pt(pet.cleanliness)));
    // Name the ailment, its remedy AND the progress: "sick" alone tells the
    // player nothing, and a static "weak from hunger" next to a full bowl
    // felt like a bug rather than a convalescence.
    for a in crate::ailment::active(pet) {
        let (label, advice) = a.describe(pet);
        out.push_str(&format!("  ! {label} — {advice}\n"));
    }
    out.push_str(&format!("  age {age_hours}h"));
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
    // `drop_stat` clamps a bottomed stat to exactly 0.0, so `<= 0.0` is exact.
    let bottomed = pet.fullness <= 0.0
        || pet.happiness <= 0.0
        || pet.energy <= 0.0
        || pet.cleanliness <= 0.0;
    if pet.sick || bottomed {
        return "critical";
    }
    if pet.sleeping {
        return "resting";
    }
    let low = f64::from(LOW);
    if pet.fullness < low || pet.happiness < low || pet.energy < low || pet.cleanliness < low {
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
        Mood::Scruffy => "^.-",
        Mood::Dirty => "x.o",
        Mood::Lonely => "._.",
        Mood::Sad => ";.;",
        Mood::Radiant => "^o^",
        Mood::Sick => "x.x",
        Mood::Sleeping => "u.u",
    };
    let mut s = format!(
        "{} ({}) f{} h{} e{} c{}",
        pet.name,
        face,
        pt(pet.fullness),
        pt(pet.happiness),
        pt(pet.energy),
        pt(pet.cleanliness)
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
        pt(pet.fullness),
        pt(pet.happiness),
        pt(pet.energy),
        pt(pet.cleanliness)
    );
    for a in crate::ailment::active(pet) {
        let (label, advice) = a.describe(pet);
        s.push_str(&format!(" It is {label} — {advice}."));
    }
    if pet.sleeping {
        s.push_str(" It is asleep right now.");
    }
    // The whole point of a moment is that the agent mentions it unprompted.
    if let Some(m) = pet.moment.as_ref().and_then(crate::moment::Active::def) {
        s.push_str(&format!(
            " Right now {} {} — worth mentioning, it does not happen often.",
            pet.name, m.label
        ));
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
    fn the_prompt_never_claims_an_expired_moment_is_happening_now() {
        // Mirrors the on_before_prompt_build path, which runs decay::advance
        // WITHOUT behaviour::update. When only behaviour expired moments, the
        // agent kept telling the player what the pet is doing "right now"
        // hours after it stopped.
        let cfg = crate::config::Config::default();
        let mut p = Pet::new("Rex".into(), 0);
        p.moment = Some(crate::moment::Active { idx: 0, ends_at_ms: 60_000 });
        crate::decay::advance(&mut p, 4 * 3_600_000, &cfg);
        let s = prompt_section(&p);
        assert!(!s.contains("Right now"), "got: {s}");
    }

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
    fn display_names_the_ailment_and_flags_sleep() {
        // Showing "ILL" told the player nothing actionable; the ailment and its
        // remedy do, and they are what the three cures are for.
        let mut p = Pet::new("Rex".into(), 0);
        p.famine_ms = 99 * 3_600_000;
        let out = display(&p, 0, 0);
        assert!(out.contains("weak from hunger"), "got {out}");
        assert!(out.contains("keep feeding it"), "the remedy must be spelled out: {out}");
        assert!(out.contains("more well-spaced"), "and the progress with it: {out}");

        let mut s = Pet::new("Rex".into(), 0);
        s.sleeping = true;
        assert!(display(&s, 0, 0).contains("asleep"));
    }

    #[test]
    fn level_escalates_with_the_pets_condition() {
        let mut p = Pet::new("Rex".into(), 0);
        p.fullness = 100.0;
        p.happiness = 100.0;
        p.energy = 100.0;
        p.cleanliness = 100.0;
        assert_eq!(level(&p), "ok");

        p.happiness = f64::from(LOW) - 1.0;
        assert_eq!(level(&p), "warn", "a low stat is a warning");

        p.happiness = 0.0;
        assert_eq!(level(&p), "critical", "a bottomed stat is critical");
    }

    #[test]
    fn pt_rounds_for_display_and_clamps_the_impossible() {
        assert_eq!(pt(80.0), 80);
        assert_eq!(pt(79.99), 80);
        assert_eq!(pt(80.49), 80);
        assert_eq!(pt(0.2), 0);
        assert_eq!(pt(-3.0), 0);
        assert_eq!(pt(250.0), 100);
    }

    #[test]
    fn illness_is_always_critical_even_with_perfect_stats() {
        let mut p = Pet::new("Rex".into(), 0);
        p.fullness = 100.0;
        p.happiness = 100.0;
        p.energy = 100.0;
        p.cleanliness = 100.0;
        p.sick = true;
        assert_eq!(level(&p), "critical");
    }

    #[test]
    fn sleeping_reads_as_resting_unless_something_is_actually_wrong() {
        let mut p = Pet::new("Rex".into(), 0);
        p.sleeping = true;
        assert_eq!(level(&p), "resting");

        // Asleep is no excuse for starving.
        p.fullness = 0.0;
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
        p.fullness = 80.0;
        p.happiness = 80.0;
        p.energy = 80.0;
        p.cleanliness = 80.0;
        assert!(compact(&p).contains("^.^"), "thriving pet should smile");

        p.fullness = 100.0;
        p.happiness = 100.0;
        p.energy = 100.0;
        p.cleanliness = 100.0;
        assert!(compact(&p).contains("^o^"), "a radiant pet beams");

        p.fullness = 5.0;
        assert!(compact(&p).contains("O.O"), "hungry pet should look alarmed");
    }

    #[test]
    fn prompt_section_states_the_facts_and_names_the_ailment() {
        let mut p = Pet::new("Rex".into(), 0);
        let normal = prompt_section(&p);
        assert!(normal.contains("Rex"));
        assert!(normal.contains("80/100"));
        assert!(normal.contains("pet_feed"));
        assert!(!normal.contains("sunk in gloom"));

        // The agent can only give useful advice if the prompt says which
        // ailment it is — "sick" would have it guessing.
        p.gloom_ms = 99 * 3_600_000;
        p.happiness = 10.0; // genuinely low, so the label is not "recovering"
        let ill = prompt_section(&p);
        assert!(ill.contains("sunk in gloom"), "got {ill}");
        assert!(ill.contains("keep playing with it"), "got {ill}");
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

