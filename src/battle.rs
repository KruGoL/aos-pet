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
    /// Percentage tweaks applied to the player's own stat line, so opponents
    /// are always in the same league — challenging, never hopeless.
    pub hp_pct: i16,
    pub attack_pct: i16,
    pub defense_pct: i16,
    pub speed_pct: i16,
    pub taunt: &'static str,
}

pub const ARCHETYPES: &[Archetype] = &[
    Archetype { name: "an alley cat", hp_pct: 0, attack_pct: 0, defense_pct: 0, speed_pct: 10,
        taunt: "sizes you up with total indifference" },
    Archetype { name: "a yard dog", hp_pct: 30, attack_pct: -10, defense_pct: 25, speed_pct: -25,
        taunt: "plants itself and refuses to move" },
    Archetype { name: "a magpie", hp_pct: -30, attack_pct: 15, defense_pct: -20, speed_pct: 45,
        taunt: "is already somewhere else" },
    Archetype { name: "an old tom", hp_pct: -10, attack_pct: 35, defense_pct: -25, speed_pct: 0,
        taunt: "has done this many times before" },
    Archetype { name: "a hedgehog", hp_pct: 10, attack_pct: -30, defense_pct: 50, speed_pct: -20,
        taunt: "rolls up and waits" },
    Archetype { name: "a stray pup", hp_pct: -20, attack_pct: -20, defense_pct: -10, speed_pct: 5,
        taunt: "wags its tail, apparently unaware this is a fight" },
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

fn tweak(base: u16, pct: i16) -> u16 {
    let adjusted = i32::from(base) + i32::from(base) * i32::from(pct) / 100;
    adjusted.clamp(1, 500) as u16
}

#[must_use]
pub fn opponent_stats(mine: Stats, arch: &Archetype) -> Stats {
    Stats {
        hp: tweak(mine.hp, arch.hp_pct),
        attack: tweak(mine.attack, arch.attack_pct),
        defense: tweak(mine.defense, arch.defense_pct),
        speed: tweak(mine.speed, arch.speed_pct),
    }
}

/// Deterministic rolls from the fight's seed. Small and self-contained so the
/// whole battle replays identically in a test.
struct Rng(u32);

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
    let swing = rng.below(5); // 0..4
    let mut dmg = i32::from(attacker) + i32::from(swing as u16) - i32::from(defender_def) / 2;
    if crit {
        dmg = dmg * 3 / 2;
    }
    (dmg.max(1) as u16, crit)
}

/// Fight a generated opponent. Pure: same seed, same fight.
#[must_use]
pub fn fight(pet_name: &str, mine: Stats, seed: u32) -> Report {
    let mut rng = Rng(seed ^ 0x9E37_79B9);
    let arch = &ARCHETYPES[(seed as usize >> 3) % ARCHETYPES.len()];
    let foe = opponent_stats(mine, arch);

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
    fn opponents_scale_with_the_pet_rather_than_being_fixed() {
        let weak = Stats { hp: 40, attack: 6, defense: 4, speed: 5 };
        let strong = Stats { hp: 85, attack: 20, defense: 16, speed: 16 };
        let arch = &ARCHETYPES[0];
        assert!(opponent_stats(strong, arch).attack > opponent_stats(weak, arch).attack);
    }
}
