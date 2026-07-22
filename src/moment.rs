//! Rare moments — the thing that makes the pet feel alive rather than tended.
//!
//! A moment is a temporary, named condition that happens *on its own*: you did
//! not cause it and you cannot summon it. The schedule is a hidden wall-clock
//! deadline plus a seed drawn in advance, both living in KV and never returned
//! by any tool, so no amount of tool-spamming can trigger, repeat or steer one.
//!
//! Everything below is one engine over a data table. Adding a nineteenth moment
//! is a row plus two faces — the complexity lives in the machinery, not the
//! count, which is why the roster can be generous.

use serde::{Deserialize, Serialize};

/// Shortest and longest gap between moments, in pet-hours.
pub const GAP_MIN_H: f64 = 3.0;
pub const GAP_SPAN_H: f64 = 9.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Any condition at all.
    Always,
    /// Needs energy to burn.
    Energetic,
    /// Only when winding down.
    Calm,
    /// Only when the pet is unhappy — these are the sad ones.
    Neglected,
    /// Only when everything is going well.
    Thriving,
}

pub struct MomentDef {
    pub key: &'static str,
    /// Shown as "<name> <label>", e.g. "Rex has the zoomies".
    pub label: &'static str,
    /// Relative frequency. Higher is commoner.
    pub weight: u16,
    pub min_minutes: u32,
    pub max_minutes: u32,
    /// Decay multiplier while active: >1 burns faster, <1 is restful.
    pub decay_mult: f64,
    pub gate: Gate,
    pub face: [&'static str; 2],
    pub fx: &'static str,
}

/// The roster. Weights are relative; gates keep sad moments away from a happy
/// pet and vice versa, so what you see reflects how the pet is actually doing.
pub const MOMENTS: &[MomentDef] = &[
    // ---- energetic ----
    MomentDef { key: "zoomies", label: "has the zoomies", weight: 100,
        min_minutes: 5, max_minutes: 20, decay_mult: 1.6, gate: Gate::Energetic,
        face: ["( ^o^ )", "( ^0^ )"], fx: "~vroom~" },
    MomentDef { key: "tail_chase", label: "is chasing its own tail", weight: 90,
        min_minutes: 5, max_minutes: 15, decay_mult: 1.3, gate: Gate::Energetic,
        face: ["( o.O )", "( O.o )"], fx: "~whirl~" },
    MomentDef { key: "pounce", label: "is stalking something invisible", weight: 70,
        min_minutes: 5, max_minutes: 20, decay_mult: 1.2, gate: Gate::Energetic,
        face: ["( >.< )", "( >o< )"], fx: "...!" },
    MomentDef { key: "parkour", label: "has decided the furniture is a mountain", weight: 45,
        min_minutes: 10, max_minutes: 30, decay_mult: 1.4, gate: Gate::Energetic,
        face: ["( ^.^ )", "( ^.- )"], fx: "~hup~" },
    // ---- calm ----
    MomentDef { key: "sunbeam", label: "has found a patch of sun", weight: 100,
        min_minutes: 20, max_minutes: 90, decay_mult: 0.5, gate: Gate::Calm,
        face: ["( u.u )", "( -.- )"], fx: "\\ | /" },
    MomentDef { key: "loaf", label: "has folded itself into a loaf", weight: 90,
        min_minutes: 20, max_minutes: 80, decay_mult: 0.6, gate: Gate::Calm,
        face: ["( -.- )", "( u.u )"], fx: "[___]" },
    MomentDef { key: "staring", label: "is staring at absolutely nothing", weight: 70,
        min_minutes: 10, max_minutes: 40, decay_mult: 0.9, gate: Gate::Calm,
        face: ["( o.o )", "( o.o )"], fx: "  ..." },
    MomentDef { key: "pondering", label: "appears to be thinking very hard", weight: 50,
        min_minutes: 15, max_minutes: 45, decay_mult: 0.8, gate: Gate::Calm,
        face: ["( '.' )", "( '.- )"], fx: "  ?" },
    // ---- about you ----
    MomentDef { key: "vigil", label: "is waiting by the door for you", weight: 80,
        min_minutes: 30, max_minutes: 180, decay_mult: 1.0, gate: Gate::Neglected,
        face: ["( ._. )", "( ._. )"], fx: "  |" },
    MomentDef { key: "sulking", label: "has turned its back to you", weight: 70,
        min_minutes: 20, max_minutes: 90, decay_mult: 1.0, gate: Gate::Neglected,
        face: ["( -.- )", "( -.- )"], fx: "  <" },
    MomentDef { key: "overjoyed", label: "cannot believe how good today is", weight: 60,
        min_minutes: 10, max_minutes: 40, decay_mult: 0.7, gate: Gate::Thriving,
        face: ["( ^v^ )", "( ^_^ )"], fx: "\\o/" },
    MomentDef { key: "copycat", label: "is copying everything you do", weight: 40,
        min_minutes: 10, max_minutes: 30, decay_mult: 1.0, gate: Gate::Thriving,
        face: ["( o.o )", "( ^.^ )"], fx: "  =" },
    // ---- odd and rare ----
    MomentDef { key: "found_a_thing", label: "has found something and is very pleased", weight: 30,
        min_minutes: 15, max_minutes: 60, decay_mult: 0.9, gate: Gate::Always,
        face: ["( ^.^ )", "( ^.o )"], fx: "  *" },
    MomentDef { key: "hiccups", label: "has the hiccups", weight: 30,
        min_minutes: 5, max_minutes: 15, decay_mult: 1.0, gate: Gate::Always,
        face: ["( o.o )", "( O.o )"], fx: " hic!" },
    MomentDef { key: "sleeptalk", label: "is talking in its sleep", weight: 25,
        min_minutes: 10, max_minutes: 40, decay_mult: 0.6, gate: Gate::Calm,
        face: ["( u.u )", "( u.o )"], fx: " ...?" },
    MomentDef { key: "the_wall", label: "is convinced something lives behind the wall", weight: 20,
        min_minutes: 10, max_minutes: 45, decay_mult: 1.1, gate: Gate::Always,
        face: ["( o.o )", "( o.O )"], fx: "  ||" },
    MomentDef { key: "singing", label: "is singing, after a fashion", weight: 15,
        min_minutes: 5, max_minutes: 20, decay_mult: 1.0, gate: Gate::Always,
        face: ["( o.o )", "( O.O )"], fx: " ~la~" },
    MomentDef { key: "philosopher", label: "has questions about the nature of the bowl", weight: 10,
        min_minutes: 20, max_minutes: 60, decay_mult: 0.9, gate: Gate::Always,
        face: ["( -.o )", "( o.- )"], fx: "  ??" },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Active {
    /// Index into `MOMENTS`. Stored rather than the key to keep saves compact;
    /// `def()` bounds-checks so a reordered table can never panic.
    pub idx: u16,
    pub ends_at_ms: u64,
}

impl Active {
    #[must_use]
    pub fn def(&self) -> Option<&'static MomentDef> {
        MOMENTS.get(self.idx as usize)
    }
}

/// Which moments a pet's condition currently allows.
#[must_use]
pub fn eligible(fullness: f64, happiness: f64, energy: f64, cleanliness: f64) -> Vec<u16> {
    let thriving = fullness >= 70.0 && happiness >= 70.0 && energy >= 70.0 && cleanliness >= 70.0;
    let neglected = happiness < 30.0;
    let energetic = energy >= 60.0;

    MOMENTS
        .iter()
        .enumerate()
        .filter(|(_, m)| match m.gate {
            Gate::Always => true,
            Gate::Energetic => energetic,
            Gate::Calm => !energetic,
            Gate::Neglected => neglected,
            Gate::Thriving => thriving,
        })
        .map(|(i, _)| i as u16)
        .collect()
}

/// Pick from the eligible set by weight, using the seed drawn when the moment
/// was scheduled. Deterministic given the seed, which is what makes the choice
/// un-steerable: it was decided before the deadline arrived.
#[must_use]
pub fn pick(eligible: &[u16], seed: u32) -> Option<u16> {
    if eligible.is_empty() {
        return None;
    }
    let total: u32 = eligible
        .iter()
        .filter_map(|i| MOMENTS.get(*i as usize))
        .map(|m| u32::from(m.weight))
        .sum();
    if total == 0 {
        return eligible.first().copied();
    }
    let mut roll = seed % total;
    for idx in eligible {
        let w = MOMENTS.get(*idx as usize).map_or(0, |m| u32::from(m.weight));
        if roll < w {
            return Some(*idx);
        }
        roll -= w;
    }
    eligible.last().copied()
}

/// How long this moment should run, derived from the same seed.
#[must_use]
pub fn duration_ms(idx: u16, seed: u32) -> u64 {
    let Some(m) = MOMENTS.get(idx as usize) else {
        return 0;
    };
    let span = m.max_minutes.saturating_sub(m.min_minutes).max(1);
    // A different slice of the seed than `pick` uses, so length and choice are
    // not correlated.
    let minutes = m.min_minutes + ((seed >> 8) % span);
    u64::from(minutes) * 60_000
}

/// Gap until the next moment, in wall-clock ms, from the same seed.
#[must_use]
pub fn next_gap_ms(seed: u32, scale: f64) -> u64 {
    let frac = f64::from((seed >> 16) & 0xFFFF) / f64::from(0xFFFFu16);
    let hours = GAP_MIN_H + frac * GAP_SPAN_H;
    // Divide by scale: compressed time should bring moments sooner in real
    // seconds, exactly as it brings decay sooner.
    let ms = (hours * 3_600_000.0) / scale.max(0.0001);
    ms as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roster_is_well_formed() {
        assert!(MOMENTS.len() >= 15, "the point is a generous roster");
        for m in MOMENTS {
            assert!(!m.key.is_empty());
            assert!(!m.label.is_empty());
            assert!(m.weight > 0, "{} would never fire", m.key);
            assert!(m.min_minutes <= m.max_minutes, "{}", m.key);
            assert!(m.decay_mult > 0.0 && m.decay_mult < 3.0, "{}", m.key);
            assert!(m.face[0].is_ascii() && m.face[1].is_ascii(), "{}", m.key);
            assert!(m.fx.is_ascii(), "{}", m.key);
        }
    }

    #[test]
    fn keys_are_unique_so_the_collection_can_count_them() {
        let mut keys: Vec<_> = MOMENTS.iter().map(|m| m.key).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), before, "duplicate moment key");
    }

    #[test]
    fn gates_keep_sad_moments_away_from_a_thriving_pet() {
        let happy = eligible(90.0, 90.0, 90.0, 90.0);
        let keys: Vec<_> = happy
            .iter()
            .filter_map(|i| MOMENTS.get(*i as usize))
            .map(|m| m.key)
            .collect();
        assert!(!keys.contains(&"vigil"), "a thriving pet is not waiting by the door");
        assert!(!keys.contains(&"sulking"));
        assert!(keys.contains(&"zoomies"), "it has the energy for it");
    }

    #[test]
    fn a_neglected_pet_can_have_the_sad_moments() {
        let sad = eligible(10.0, 10.0, 10.0, 10.0);
        let keys: Vec<_> = sad
            .iter()
            .filter_map(|i| MOMENTS.get(*i as usize))
            .map(|m| m.key)
            .collect();
        assert!(keys.contains(&"vigil"));
        assert!(!keys.contains(&"overjoyed"), "nothing to be overjoyed about");
        assert!(!keys.contains(&"zoomies"), "no energy to burn");
    }

    #[test]
    fn every_moment_is_reachable_from_some_condition() {
        let mut seen: Vec<&str> = Vec::new();
        for (f, h, e, c) in [
            (90.0, 90.0, 90.0, 90.0),
            (10.0, 10.0, 10.0, 10.0),
            (50.0, 50.0, 20.0, 50.0),
            (50.0, 50.0, 90.0, 50.0),
        ] {
            for i in eligible(f, h, e, c) {
                if let Some(m) = MOMENTS.get(i as usize) {
                    seen.push(m.key);
                }
            }
        }
        for m in MOMENTS {
            assert!(seen.contains(&m.key), "{} can never happen", m.key);
        }
    }

    #[test]
    fn pick_always_returns_something_eligible() {
        let set = eligible(50.0, 50.0, 50.0, 50.0);
        assert!(!set.is_empty());
        for seed in [0u32, 1, 7, 99, 12345, u32::MAX] {
            let got = pick(&set, seed).expect("must pick");
            assert!(set.contains(&got), "seed {seed} picked outside the set");
        }
    }

    #[test]
    fn pick_on_an_empty_set_is_none_not_a_panic() {
        assert_eq!(pick(&[], 42), None);
    }

    #[test]
    fn weights_actually_bias_the_outcome() {
        let set = eligible(50.0, 50.0, 50.0, 50.0);
        let mut sunbeam = 0;
        let mut philosopher = 0;
        for seed in 0..4000u32 {
            match pick(&set, seed).and_then(|i| MOMENTS.get(i as usize)).map(|m| m.key) {
                Some("sunbeam") => sunbeam += 1,
                Some("philosopher") => philosopher += 1,
                _ => {}
            }
        }
        assert!(
            sunbeam > philosopher * 3,
            "weight 100 should dominate weight 10: {sunbeam} vs {philosopher}"
        );
    }

    #[test]
    fn duration_stays_inside_the_declared_window() {
        for (i, m) in MOMENTS.iter().enumerate() {
            for seed in [0u32, 5, 777, u32::MAX] {
                let ms = duration_ms(i as u16, seed);
                let minutes = ms / 60_000;
                assert!(
                    minutes >= u64::from(m.min_minutes) && minutes <= u64::from(m.max_minutes),
                    "{} produced {minutes}min outside {}..{}",
                    m.key, m.min_minutes, m.max_minutes
                );
            }
        }
    }

    #[test]
    fn an_out_of_range_index_is_survived() {
        assert_eq!(duration_ms(9999, 1), 0);
        assert!(Active { idx: 9999, ends_at_ms: 0 }.def().is_none());
    }

    #[test]
    fn gaps_land_in_the_declared_band_and_shrink_with_compressed_time() {
        for seed in [0u32, 12345, u32::MAX] {
            let h = next_gap_ms(seed, 1.0) as f64 / 3_600_000.0;
            assert!(h >= GAP_MIN_H - 0.01 && h <= GAP_MIN_H + GAP_SPAN_H + 0.01, "got {h}h");
        }
        assert!(
            next_gap_ms(12345, 60.0) < next_gap_ms(12345, 1.0),
            "compressed time must bring moments sooner in real seconds"
        );
    }
}
