//! Three ailments, each earned a different way and each needing a different
//! remedy — so the player has to read the symptom instead of pressing one
//! button.
//!
//! The design trick is that **the cure undoes the cause**: a pet that starved
//! into Famine is fed back out of it, one that sat filthy is washed out of it.
//! `pet_heal` is real medicine but not a master key — it eases every ailment a
//! little and cures none outright, so it buys time rather than replacing care.
//!
//! Counters accumulate with `+= elapsed` once per span, never per period, so a
//! pet left alone for a year costs exactly as much as one checked a second ago.

use serde::{Deserialize, Serialize};

use crate::config::{Config, LOW, MS_PER_HOUR};
use crate::model::Pet;

/// Pet-hours of neglect before an ailment sets in.
pub const ONSET_H: f64 = 6.0;
/// Pet-hours of neglect a correct remedy removes.
pub const REMEDY_H: f64 = 2.5;
/// What medicine takes off every ailment — help, not a cure.
pub const MEDICINE_H: f64 = 1.0;
/// How deep an ailment may ever get, in pet-hours.
///
/// Without a ceiling the clocks are unbounded, and neglect converts directly
/// into cure-time at 1:1 — a pet left a week needs 67 perfectly-spaced meals,
/// which at four hours of readiness apiece is eleven real days. That is a death
/// sentence served slowly, and the game's one promise is that the pet never
/// dies. Capping at three times the onset keeps serious neglect genuinely
/// serious (~8 well-timed remedies) while keeping every illness escapable.
pub const MAX_DEPTH_H: f64 = ONSET_H * 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ailment {
    /// Starved too long. Fed back to health.
    Famine,
    /// Left filthy too long. Washed back to health.
    Grime,
    /// Nobody played with it for days. Medicine does not touch this one.
    Gloom,
}

impl Ailment {
    /// Stable machine-readable key for clients that want to branch on it.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Famine => "famine",
            Self::Grime => "grime",
            Self::Gloom => "gloom",
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Famine => "weak from hunger",
            Self::Grime => "itchy and sore from the dirt",
            Self::Gloom => "sunk in gloom",
        }
    }

    /// What the player should actually do — the whole point of having three.
    #[must_use]
    pub fn remedy(self) -> &'static str {
        match self {
            Self::Famine => "feed it regularly until it recovers",
            Self::Grime => "wash it, more than once",
            Self::Gloom => "play with it — medicine will not lift this",
        }
    }
}

fn hours_to_ms(h: f64) -> u64 {
    (h * MS_PER_HOUR) as u64
}

/// Accrue neglect for a span. Called once per `advance`, never in a loop.
pub fn accrue(pet: &mut Pet, elapsed_ms: u64, cfg: &Config) {
    let scaled = (elapsed_ms as f64 * cfg.scale) as u64;
    // The ceiling is applied on write, so a pet that was already driven past it
    // — by a long absence or a compressed-time demo — is pulled back into
    // recoverable range on its very next tick rather than staying doomed.
    let cap = hours_to_ms(MAX_DEPTH_H);

    if pet.fullness == 0 {
        pet.famine_ms = pet.famine_ms.saturating_add(scaled).min(cap);
    }
    if pet.cleanliness == 0 {
        pet.grime_ms = pet.grime_ms.saturating_add(scaled).min(cap);
    }
    // Gloom is about a sustained low mood rather than an empty bar — you can be
    // fed and clean and still be miserable.
    if pet.happiness < LOW {
        pet.gloom_ms = pet.gloom_ms.saturating_add(scaled).min(cap);
    } else {
        // Cheer it up at all and the clock starts unwinding on its own.
        pet.gloom_ms = pet.gloom_ms.saturating_sub(scaled);
    }

    // Clamp on EVERY pass, not only while accruing. A pet that arrives already
    // past the ceiling — saved before the cap existed, or run through a
    // compressed-time demo — has full bars and so never enters the branches
    // above; without this it would stay doomed forever.
    pet.famine_ms = pet.famine_ms.min(cap);
    pet.grime_ms = pet.grime_ms.min(cap);
    pet.gloom_ms = pet.gloom_ms.min(cap);
}

/// Which ailments have set in.
#[must_use]
pub fn active(pet: &Pet) -> Vec<Ailment> {
    let onset = hours_to_ms(ONSET_H);
    let mut out = Vec::new();
    if pet.famine_ms >= onset {
        out.push(Ailment::Famine);
    }
    if pet.grime_ms >= onset {
        out.push(Ailment::Grime);
    }
    if pet.gloom_ms >= onset {
        out.push(Ailment::Gloom);
    }
    out
}

#[must_use]
pub fn is_ill(pet: &Pet) -> bool {
    !active(pet).is_empty()
}

/// The ailment that makes play impossible, if any.
///
/// Deliberately never `Gloom`: playing is the *cure* for gloom, so blocking it
/// on a blanket "is sick" check would leave the player with no way out.
#[must_use]
pub fn blocks_play(pet: &Pet) -> Option<Ailment> {
    active(pet).into_iter().find(|a| *a != Ailment::Gloom)
}

/// A correct remedy walks the matching counter back.
///
/// `strength` is the same 0..=1 readiness the happiness economy uses, so
/// recovery follows the same rule as everything else: care spread out counts,
/// hammering the same button three times in a row does not. Without this you
/// could cure a week of starvation with three clicks.
pub fn apply_remedy(pet: &mut Pet, kind: Ailment, strength: f64) {
    let step = (hours_to_ms(REMEDY_H) as f64 * strength.clamp(0.0, 1.0)) as u64;
    match kind {
        Ailment::Famine => pet.famine_ms = pet.famine_ms.saturating_sub(step),
        Ailment::Grime => pet.grime_ms = pet.grime_ms.saturating_sub(step),
        Ailment::Gloom => pet.gloom_ms = pet.gloom_ms.saturating_sub(step),
    }
}

/// Medicine: eases everything a little, cures nothing on its own. Gloom is
/// deliberately untouched — you cannot medicate loneliness away.
///
/// Takes `strength` for the same reason the remedies do: without it, `pet_heal`
/// is the one action in the capsule with no clock, and twenty calls in a loop
/// clear twenty hours of neglect. That would make medicine the master key the
/// whole three-ailment design exists to rule out.
pub fn apply_medicine(pet: &mut Pet, strength: f64) {
    let step = (hours_to_ms(MEDICINE_H) as f64 * strength.clamp(0.0, 1.0)) as u64;
    pet.famine_ms = pet.famine_ms.saturating_sub(step);
    pet.grime_ms = pet.grime_ms.saturating_sub(step);
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: u64 = 3_600_000;

    fn pet() -> Pet {
        let mut p = Pet::new("Rex".into(), 0);
        p.happiness = 80; // above LOW, so gloom does not accrue by accident
        p
    }

    #[test]
    fn starving_earns_famine_and_nothing_else() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 7 * HOUR, &cfg);
        assert_eq!(active(&p), vec![Ailment::Famine]);
    }

    #[test]
    fn filth_earns_grime() {
        let cfg = Config::default();
        let mut p = pet();
        p.cleanliness = 0;
        accrue(&mut p, 7 * HOUR, &cfg);
        assert_eq!(active(&p), vec![Ailment::Grime]);
    }

    #[test]
    fn a_low_mood_earns_gloom_even_with_every_other_need_met() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 100;
        p.cleanliness = 100;
        p.happiness = LOW - 1;
        accrue(&mut p, 7 * HOUR, &cfg);
        assert_eq!(active(&p), vec![Ailment::Gloom]);
    }

    #[test]
    fn ailments_stack_when_everything_was_neglected() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        p.cleanliness = 0;
        p.happiness = 0;
        accrue(&mut p, 7 * HOUR, &cfg);
        assert_eq!(active(&p).len(), 3, "a truly abandoned pet is not just 'sick'");
    }

    #[test]
    fn nothing_sets_in_below_the_onset_threshold() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 5 * HOUR, &cfg);
        assert!(active(&p).is_empty(), "one missed meal is not an illness");
    }

    #[test]
    fn the_right_remedy_cures_and_the_wrong_one_does_nothing() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 8 * HOUR, &cfg);
        assert!(active(&p).contains(&Ailment::Famine));

        // Washing a starving pet is kind but beside the point.
        apply_remedy(&mut p, Ailment::Grime, 1.0);
        assert!(active(&p).contains(&Ailment::Famine), "wrong remedy must not cure");

        apply_remedy(&mut p, Ailment::Famine, 1.0);
        assert!(!active(&p).contains(&Ailment::Famine), "the right one does");
    }

    #[test]
    fn a_spammed_remedy_is_worth_almost_nothing() {
        // Regression guard: if readiness were ignored here, three rapid clicks
        // would undo days of neglect and the ailment system would be theatre.
        let cfg = Config::default();
        let mut spammed = pet();
        spammed.fullness = 0;
        accrue(&mut spammed, 20 * HOUR, &cfg);
        let mut patient = spammed.clone();

        for _ in 0..3 {
            apply_remedy(&mut spammed, Ailment::Famine, 0.05);
        }
        apply_remedy(&mut patient, Ailment::Famine, 1.0);

        assert!(
            patient.famine_ms < spammed.famine_ms,
            "one well-timed meal must beat three impatient ones"
        );
        assert!(active(&spammed).contains(&Ailment::Famine));
    }

    #[test]
    fn medicine_helps_but_never_cures_alone() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 9 * HOUR, &cfg);
        let before = p.famine_ms;

        apply_medicine(&mut p, 1.0);
        assert!(p.famine_ms < before, "medicine should ease it");
        assert!(
            active(&p).contains(&Ailment::Famine),
            "one dose must not replace feeding the pet"
        );
    }

    #[test]
    fn medicine_cannot_touch_gloom() {
        let cfg = Config::default();
        let mut p = pet();
        p.happiness = 0;
        accrue(&mut p, 9 * HOUR, &cfg);
        let before = p.gloom_ms;
        apply_medicine(&mut p, 1.0);
        assert_eq!(p.gloom_ms, before, "you cannot medicate loneliness away");
    }

    #[test]
    fn cheering_the_pet_up_unwinds_gloom_by_itself() {
        let cfg = Config::default();
        let mut p = pet();
        p.happiness = 0;
        accrue(&mut p, 9 * HOUR, &cfg);
        assert!(active(&p).contains(&Ailment::Gloom));

        p.happiness = 90;
        accrue(&mut p, 9 * HOUR, &cfg);
        assert!(!active(&p).contains(&Ailment::Gloom), "kept company, it lifts");
    }

    #[test]
    fn a_year_of_neglect_costs_one_addition_not_a_loop() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 365 * 24 * HOUR, &cfg);
        assert!(active(&p).contains(&Ailment::Famine));
        assert!(p.famine_ms > 0, "and it saturates rather than overflowing");
    }

    #[test]
    fn no_amount_of_neglect_makes_a_pet_incurable() {
        // The promise is that the pet never dies. An unbounded clock breaks it
        // quietly: a year away once meant ~3500 perfectly-spaced meals to undo,
        // which is a death sentence wearing a recovery costume.
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 365 * 24 * HOUR, &cfg);

        let mut remedies = 0;
        while active(&p).contains(&Ailment::Famine) {
            apply_remedy(&mut p, Ailment::Famine, 1.0);
            remedies += 1;
            assert!(remedies < 50, "recovery must be reachable, not theoretical");
        }
        assert!(remedies <= 8, "took {remedies} well-timed meals");
    }

    #[test]
    fn an_already_doomed_pet_is_pulled_back_into_range() {
        // Pets saved before the ceiling existed (or run through a compressed-time
        // demo) carry huge clocks. The cap is applied on write so they recover on
        // the next tick instead of staying permanently ill.
        let cfg = Config::default();
        let mut p = pet();
        // Full bars on purpose: the doomed pet observed live had fullness 100,
        // so the accrual branches never ran and an in-branch clamp missed it.
        p.fullness = 100;
        p.cleanliness = 100;
        p.famine_ms = 743 * HOUR; // an actual value observed in a live pet
        p.grime_ms = 743 * HOUR;
        accrue(&mut p, 1000, &cfg);
        assert!(p.grime_ms <= hours_to_ms(MAX_DEPTH_H), "grime too");
        assert!(
            p.famine_ms <= hours_to_ms(MAX_DEPTH_H),
            "legacy depth must be clamped, got {}",
            p.famine_ms
        );
    }

    #[test]
    fn compressed_time_makes_illness_arrive_sooner_in_real_seconds() {
        let fast = Config::default().with_scale_str("2000");
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 20_000, &fast); // 20 real seconds
        assert!(active(&p).contains(&Ailment::Famine), "demos must be possible");
    }

    #[test]
    fn spamming_medicine_is_not_a_master_key() {
        // Regression guard: twenty ungated doses used to clear twenty pet-hours
        // of neglect at once, which made every other cure pointless.
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 20 * HOUR, &cfg);
        let before = p.famine_ms;

        for _ in 0..20 {
            apply_medicine(&mut p, 0.02);
        }
        assert!(
            p.famine_ms > before - hours_to_ms(MEDICINE_H),
            "twenty impatient doses must be worth less than one patient one"
        );
        assert!(active(&p).contains(&Ailment::Famine), "and must not cure");
    }

    #[test]
    fn a_gloomy_pet_can_still_be_played_with() {
        // The deadlock guard: gloom is cured by play, so if play were refused
        // on a blanket illness check the pet could never be cheered up again.
        let cfg = Config::default();
        let mut p = pet();
        p.happiness = 0;
        accrue(&mut p, 9 * HOUR, &cfg);
        assert!(is_ill(&p), "it is genuinely unwell");
        assert_eq!(blocks_play(&p), None, "and yet play must stay available");
    }

    #[test]
    fn a_physically_ill_pet_is_too_weak_to_play() {
        let cfg = Config::default();
        let mut p = pet();
        p.fullness = 0;
        accrue(&mut p, 9 * HOUR, &cfg);
        assert_eq!(blocks_play(&p), Some(Ailment::Famine));
    }

    #[test]
    fn every_ailment_names_a_different_remedy() {
        let all = [Ailment::Famine, Ailment::Grime, Ailment::Gloom];
        let mut remedies: Vec<_> = all.iter().map(|a| a.remedy()).collect();
        remedies.sort_unstable();
        remedies.dedup();
        assert_eq!(remedies.len(), 3, "the point is that they differ");
    }
}
