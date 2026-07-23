//! Friendly scraps against procedurally generated opponents.
//!
//! Combat stats are **derived from care**, never trained. A pet that is fed,
//! rested and looked after fights well; a neglected one loses. That keeps the
//! battle layer expressing the care loop instead of competing with it — there
//! is no separate grind that would make feeding the pet optional.
//!
//! Every roll happens here, inside the sandbox, from a seed the caller drew
//! before the fight. The model narrating the fight cannot nudge its outcome.

use crate::model::Pet;

pub const MAX_ROUNDS: u8 = 8;
/// A round costs the pet real energy, so a tired pet simply cannot fight —
/// a cooldown that needs no timer and reads as common sense.
pub const ENERGY_COST: u8 = 20;
pub const MIN_ENERGY: u8 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub hp: u16,
    pub attack: u16,
    pub defense: u16,
    pub speed: u16,
}

pub struct Archetype {
    pub name: &'static str,
    /// Fixed stat line: this is what the creature IS, regardless of who walks
    /// up to it. Opponents used to mirror the player's own stats scaled by a
    /// percentage, which quietly made care worthless in a fight — a filthy,
    /// exhausted pet met proportionally feeble opponents and won exactly as
    /// often as a cherished one. Absolute lines are what let care shift odds.
    pub hp: u16,
    pub attack: u16,
    pub defense: u16,
    pub speed: u16,
    pub taunt: &'static str,
}

/// The field, roughly in ascending order of menace. Tuned against the
/// simulated win-rate table in the tests below: a cherished stage-3 pet
/// should beat the field comfortably but find the yard dog a genuine wall,
/// and a neglected pet should scrape the odd win off the pup and little else.
pub const ARCHETYPES: &[Archetype] = &[
    Archetype { name: "a stray pup", hp: 32, attack: 7, defense: 4, speed: 9,
        taunt: "wags its tail, apparently unaware this is a fight" },
    Archetype { name: "an alley cat", hp: 58, attack: 14, defense: 10, speed: 13,
        taunt: "sizes you up with total indifference" },
    Archetype { name: "a magpie", hp: 40, attack: 18, defense: 5, speed: 20,
        taunt: "is already somewhere else" },
    Archetype { name: "a hedgehog", hp: 70, attack: 10, defense: 20, speed: 6,
        taunt: "rolls up and waits" },
    Archetype { name: "an old tom", hp: 58, attack: 16, defense: 8, speed: 12,
        taunt: "has done this many times before" },
    Archetype { name: "a yard dog", hp: 120, attack: 17, defense: 18, speed: 8,
        taunt: "plants itself and refuses to move" },
];

/// Growth stage derived from age alone: unmissable, unfarmable, and needing no
/// extra state to track.
#[must_use]
pub fn stage(age_ms: u64) -> u16 {
    const DAY: u64 = 86_400_000;
    match age_ms {
        a if a >= 30 * DAY => 3,
        a if a >= 7 * DAY => 2,
        a if a >= DAY => 1,
        _ => 0,
    }
}

/// Turn care into fighting ability. This mapping is the design's keystone.
#[must_use]
pub fn stats_for(pet: &Pet, now_ms: u64) -> Stats {
    let st = stage(pet.age_ms(now_ms));
    // Stats carry fractions internally; a fight works on whole points, so
    // round at this boundary exactly as the views do.
    Stats {
        hp: 40 + st * 15,
        attack: 5 + (pet.happiness.round() as u16) / 10 + (pet.energy.round() as u16) / 20,
        defense: 3 + (pet.cleanliness.round() as u16) / 12 + if pet.sick { 0 } else { 5 },
        speed: 4 + (pet.energy.round() as u16) / 8,
    }
}

/// ±10% seeded jitter, so two magpies are not clones without ever leaving the
/// archetype's league.
fn jitter(base: u16, rng: &mut Rng) -> u16 {
    let pct = rng.below(21) as i32 - 10;
    let adjusted = i32::from(base) + i32::from(base) * pct / 100;
    adjusted.clamp(1, 500) as u16
}

#[must_use]
pub fn opponent_stats(arch: &Archetype, rng: &mut Rng) -> Stats {
    Stats {
        hp: jitter(arch.hp, rng),
        attack: jitter(arch.attack, rng),
        defense: jitter(arch.defense, rng),
        speed: jitter(arch.speed, rng),
    }
}

/// Deterministic rolls from the fight's seed. Small and self-contained so the
/// whole battle replays identically in a test.
pub struct Rng(pub u32);

impl Rng {
    fn next(&mut self) -> u32 {
        // xorshift32 — plenty for damage variance.
        let mut x = self.0 | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub opponent: String,
    pub taunt: String,
    pub log: Vec<String>,
    pub won: bool,
    /// True when nobody fell inside the round limit and it was decided on the
    /// remaining health share.
    pub on_points: bool,
    pub my_hp_left: u16,
    pub foe_hp_left: u16,
}

fn strike(attacker: u16, defender_def: u16, rng: &mut Rng) -> (u16, bool) {
    let crit = rng.below(100) < 12;
    // Swing scales with attack and is centred on zero, so the mean stays
    // attack − def/2 while single blows vary a lot. A flat ±4 made every fight
    // a threshold function of the stat lines: the favourite won every replay,
    // the underdog none, and one point of defense flipped entire seed sets.
    let spread = u32::from(attacker) / 2 + 4;
    let swing = rng.below(spread) as i32 - (spread as i32) / 2;
    let mut dmg = i32::from(attacker) + swing - i32::from(defender_def) / 2;
    if crit {
        dmg = dmg * 3 / 2;
    }
    (dmg.max(1) as u16, crit)
}

/// Fight a generated opponent. Pure: same seed, same fight.
#[must_use]
pub fn fight(pet_name: &str, mine: Stats, seed: u32) -> Report {
    let idx = (seed as usize >> 3) % ARCHETYPES.len();
    fight_against(pet_name, mine, idx, seed)
}

/// Fight a specific archetype. Split out so the balance simulation in the
/// tests can measure each opponent's win rate directly.
#[must_use]
pub fn fight_against(pet_name: &str, mine: Stats, idx: usize, seed: u32) -> Report {
    let mut rng = Rng(seed ^ 0x9E37_79B9);
    let arch = &ARCHETYPES[idx % ARCHETYPES.len()];
    let foe = opponent_stats(arch, &mut rng);

    let mut my_hp = i32::from(mine.hp);
    let mut foe_hp = i32::from(foe.hp);
    let mut log = Vec::new();
    let i_start = mine.speed >= foe.speed;

    for round in 1..=MAX_ROUNDS {
        // Faster fighter swings first; that is the whole reward for being rested.
        let order = if i_start { [true, false] } else { [false, true] };
        for mine_turn in order {
            if my_hp <= 0 || foe_hp <= 0 {
                break;
            }
            if mine_turn {
                let (d, crit) = strike(mine.attack, foe.defense, &mut rng);
                foe_hp -= i32::from(d);
                log.push(format!(
                    "R{round}: {pet_name} hits for {d}{}{}",
                    if crit { " (critical!)" } else { "" },
                    if foe_hp <= 0 { " — and that settles it" } else { "" }
                ));
            } else {
                let (d, crit) = strike(foe.attack, mine.defense, &mut rng);
                my_hp -= i32::from(d);
                let finisher = if my_hp <= 0 {
                    format!(" — {pet_name} has had enough")
                } else {
                    String::new()
                };
                log.push(format!(
                    "R{round}: {} hits back for {d}{}{finisher}",
                    arch.name,
                    if crit { " (critical!)" } else { "" },
                ));
            }
        }
        if my_hp <= 0 || foe_hp <= 0 {
            break;
        }
    }

    let decided = my_hp <= 0 || foe_hp <= 0;
    // Nobody fell: whoever kept the larger share of their own health wins, so a
    // tanky opponent cannot win simply by having more to lose.
    let my_share = my_hp.max(0) as f64 / f64::from(mine.hp.max(1));
    let foe_share = foe_hp.max(0) as f64 / f64::from(foe.hp.max(1));
    let won = if decided { foe_hp <= 0 && my_hp > 0 } else { my_share >= foe_share };

    if !decided {
        log.push(format!(
            "Both still standing after {MAX_ROUNDS} rounds — decided on condition"
        ));
    }

    Report {
        opponent: arch.name.to_string(),
        taunt: format!("{} {}", arch.name, arch.taunt),
        log,
        won,
        on_points: !decided,
        my_hp_left: my_hp.max(0) as u16,
        foe_hp_left: foe_hp.max(0) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400_000;

    fn pet() -> Pet {
        Pet::new("Rex".into(), 0)
    }

    #[test]
    fn stage_comes_from_age_and_cannot_be_missed() {
        assert_eq!(stage(0), 0);
        assert_eq!(stage(DAY - 1), 0);
        assert_eq!(stage(DAY), 1);
        assert_eq!(stage(7 * DAY), 2);
        assert_eq!(stage(30 * DAY), 3);
        assert_eq!(stage(3650 * DAY), 3, "caps rather than growing forever");
    }

    #[test]
    fn a_well_kept_pet_outfights_a_neglected_one() {
        let mut good = pet();
        good.happiness = 100.0;
        good.energy = 100.0;
        good.cleanliness = 100.0;

        let mut bad = pet();
        bad.happiness = 5.0;
        bad.energy = 5.0;
        bad.cleanliness = 5.0;
        bad.sick = true;

        let g = stats_for(&good, DAY);
        let b = stats_for(&bad, DAY);
        assert!(g.attack > b.attack, "care must show up as power");
        assert!(g.defense > b.defense);
        assert!(g.speed > b.speed);
    }

    #[test]
    fn illness_costs_defence_specifically() {
        let mut p = pet();
        p.cleanliness = 60.0;
        let healthy = stats_for(&p, 0).defense;
        p.sick = true;
        assert!(stats_for(&p, 0).defense < healthy);
    }

    #[test]
    fn age_is_the_only_source_of_extra_health() {
        let p = pet();
        assert!(stats_for(&p, 30 * DAY).hp > stats_for(&p, 0).hp);
        assert_eq!(stats_for(&p, 0).hp, 40);
    }

    #[test]
    fn the_same_seed_replays_the_same_fight() {
        let s = stats_for(&pet(), DAY);
        let a = fight("Rex", s, 4242);
        let b = fight("Rex", s, 4242);
        assert_eq!(a, b, "battles must be reproducible for tests and audits");
    }

    #[test]
    fn different_seeds_give_different_fights() {
        let s = stats_for(&pet(), DAY);
        let mut opponents = std::collections::BTreeSet::new();
        for seed in 0..200u32 {
            opponents.insert(fight("Rex", s, seed).opponent);
        }
        assert!(opponents.len() > 1, "every fight met the same opponent");
    }

    #[test]
    fn every_archetype_is_reachable() {
        let s = stats_for(&pet(), DAY);
        let mut seen = std::collections::BTreeSet::new();
        for seed in 0..2000u32 {
            seen.insert(fight("Rex", s, seed).opponent);
        }
        assert_eq!(seen.len(), ARCHETYPES.len(), "some opponent can never appear");
    }

    #[test]
    fn a_fight_always_terminates_and_produces_a_log() {
        let s = stats_for(&pet(), DAY);
        for seed in 0..300u32 {
            let r = fight("Rex", s, seed);
            assert!(!r.log.is_empty(), "seed {seed} produced no blows");
            assert!(r.log.len() <= (MAX_ROUNDS as usize) * 2 + 1);
        }
    }

    #[test]
    fn damage_is_never_zero_however_armoured_the_target() {
        let mut rng = Rng(1);
        for _ in 0..500 {
            let (d, _) = strike(1, 500, &mut rng);
            assert!(d >= 1, "a fight with unhittable targets would never end");
        }
    }

    #[test]
    fn a_strong_pet_usually_beats_the_field() {
        let mut good = pet();
        good.happiness = 100.0;
        good.energy = 100.0;
        good.cleanliness = 100.0;
        let s = stats_for(&good, 30 * DAY);

        let wins = (0..300u32).filter(|seed| fight("Rex", s, *seed).won).count();
        assert!(wins > 150, "a peak pet won only {wins}/300 — care should pay");
    }

    #[test]
    fn a_neglected_pet_still_sometimes_wins() {
        let mut bad = pet();
        bad.happiness = 0.0;
        bad.energy = 30.0;
        bad.cleanliness = 0.0;
        bad.sick = true;
        let s = stats_for(&bad, 0);

        let wins = (0..300u32).filter(|seed| fight("Rex", s, *seed).won).count();
        assert!(wins > 0, "hopeless fights are not fun");
    }

    #[test]
    fn opponents_are_what_they_are_regardless_of_the_challenger() {
        // The regression this guards: mirrored opponents made care worthless,
        // because a feeble pet met proportionally feeble opposition.
        let arch = &ARCHETYPES[0];
        let a = opponent_stats(arch, &mut Rng(7));
        let b = opponent_stats(arch, &mut Rng(7));
        assert_eq!(a, b, "same seed, same creature");
        assert!(a.hp >= arch.hp * 9 / 10 && a.hp <= arch.hp * 11 / 10, "jitter stays in league");
    }

    // ---- balance simulation ----
    //
    // Three care tiers, measured against every archetype over the same seed
    // set. These bands ARE the design: care must shift the odds, the yard dog
    // must stay a genuine wall even for a cherished pet, and nothing may be
    // perfectly hopeless.

    // The tiers come from the REAL care→stats mapping, not frozen constants:
    // an audit exercise proved that hand-copied stat lines let stats_for be
    // gutted (care contribution flattened to nothing) with the whole balance
    // suite still green. Deriving them binds these bands to the keystone.
    fn tier(f: f64, h: f64, e: f64, c: f64, sick: bool, age: u64) -> Stats {
        let mut p = pet();
        p.fullness = f;
        p.happiness = h;
        p.energy = e;
        p.cleanliness = c;
        p.sick = sick;
        stats_for(&p, age)
    }
    fn peak() -> Stats {
        tier(100.0, 100.0, 100.0, 100.0, false, 30 * DAY)
    }
    fn mid() -> Stats {
        tier(60.0, 60.0, 60.0, 60.0, false, DAY)
    }
    fn neglected() -> Stats {
        tier(10.0, 10.0, 30.0, 10.0, true, 0)
    }

    #[test]
    fn the_tiers_reflect_what_care_actually_buys() {
        // Sanity anchor: if this fails, stats_for changed — retune the bands
        // consciously instead of letting them drift.
        assert_eq!(peak(), Stats { hp: 85, attack: 20, defense: 16, speed: 16 });
        assert_eq!(mid(), Stats { hp: 55, attack: 14, defense: 13, speed: 11 });
        assert_eq!(neglected(), Stats { hp: 40, attack: 7, defense: 3, speed: 7 });
    }

    fn win_rate(mine: Stats, idx: usize, n: u32) -> f64 {
        let wins = (0..n)
            .filter(|s| fight_against("Rex", mine, idx, s.wrapping_mul(2_654_435_761)).won)
            .count();
        f64::from(wins as u32) / f64::from(n)
    }

    #[test]
    #[ignore = "diagnostic: prints the full win-rate table for tuning"]
    fn print_the_win_rate_table() {
        for (i, a) in ARCHETYPES.iter().enumerate() {
            println!(
                "{:<14} peak {:>5.1}%  mid {:>5.1}%  neglected {:>5.1}%",
                a.name,
                win_rate(peak(), i, 2000) * 100.0,
                win_rate(mid(), i, 2000) * 100.0,
                win_rate(neglected(), i, 2000) * 100.0,
            );
        }
    }

    #[test]
    fn care_shifts_the_odds_against_every_single_opponent() {
        for (i, a) in ARCHETYPES.iter().enumerate() {
            let p = win_rate(peak(), i, 1000);
            let m = win_rate(mid(), i, 1000);
            let n = win_rate(neglected(), i, 1000);
            // Monotone across tiers, and strictly better from bottom to top —
            // the ends may saturate (peak sweeps the pup, everyone fails the
            // dog at mid), so strictness between adjacent tiers would demand
            // impossible resolution from a 0..=100% scale.
            assert!(p >= m && m >= n, "{}: {p:.2} / {m:.2} / {n:.2} not monotone", a.name);
            assert!(p > n, "{}: care must matter, got peak {p:.2} vs neglected {n:.2}", a.name);
        }
    }

    #[test]
    fn the_yard_dog_is_a_wall_but_not_a_locked_door() {
        let idx = ARCHETYPES.iter().position(|a| a.name == "a yard dog").unwrap();
        let p = win_rate(peak(), idx, 2000);
        assert!(
            (0.20..=0.50).contains(&p),
            "a cherished pet should win sometimes and lose often: got {p:.2}"
        );
    }

    #[test]
    fn the_pup_gives_even_a_neglected_pet_the_odd_win() {
        let idx = ARCHETYPES.iter().position(|a| a.name == "a stray pup").unwrap();
        let n = win_rate(neglected(), idx, 2000);
        assert!(n > 0.05, "perfectly hopeless fights are not fun: got {n:.2}");
    }
}
