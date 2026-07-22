//! A guessing game the agent genuinely cannot cheat at.
//!
//! The secret lives in the capsule's KV, inside the sandbox. The model can see
//! the hints it produces but never the number itself — so "the LLM cannot fake
//! state" stops being a claim and becomes something you can play against.
//!
//! Randomness is injected by the caller, so every rule here stays pure and
//! deterministic under test.

use serde::{Deserialize, Serialize};

pub const KV_GAME: &str = "game";
pub const MIN: u32 = 1;
pub const MAX: u32 = 20;
pub const MAX_GUESSES: u32 = 6;
/// Starting a round tires the pet out a little, so it cannot be farmed forever.
pub const ENERGY_COST: u8 = 10;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    pub secret: u32,
    pub guesses: u32,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The secret is lower than the guess.
    Lower,
    /// The secret is higher than the guess.
    Higher,
    Won { guesses: u32, reward: u8 },
    /// Out of attempts; the round is over and the secret is revealed.
    Lost { secret: u32 },
    OutOfRange,
}

impl Game {
    #[must_use]
    pub fn new(secret: u32, now_ms: u64) -> Self {
        Self {
            secret: secret.clamp(MIN, MAX),
            guesses: 0,
            started_at_ms: now_ms,
        }
    }

    #[must_use]
    pub fn guesses_left(&self) -> u32 {
        MAX_GUESSES.saturating_sub(self.guesses)
    }
}

/// Map raw random bytes onto the playable range.
///
/// Modulo bias is irrelevant for a 20-value game, and being unbiased here would
/// cost a rejection loop for no gain.
#[must_use]
pub fn secret_from_bytes(bytes: [u8; 4]) -> u32 {
    let raw = u32::from_le_bytes(bytes);
    MIN + (raw % (MAX - MIN + 1))
}

/// Fewer guesses, bigger reward — but never nothing for finishing.
#[must_use]
pub fn reward_for(guesses: u32) -> u8 {
    let base: u32 = 34;
    base.saturating_sub(guesses.saturating_mul(4)).clamp(10, 30) as u8
}

/// Flavour text so a near miss feels different from a wild stab.
#[must_use]
pub fn warmth(secret: u32, guess: u32) -> &'static str {
    match secret.abs_diff(guess) {
        0 => "spot on",
        1..=2 => "burning hot",
        3..=4 => "warm",
        5..=8 => "cool",
        _ => "freezing",
    }
}

/// Apply a guess. The caller persists the mutated game (or clears it when the
/// round ends).
pub fn guess(game: &mut Game, value: u32) -> Outcome {
    if !(MIN..=MAX).contains(&value) {
        // A bad input must not burn an attempt.
        return Outcome::OutOfRange;
    }
    game.guesses = game.guesses.saturating_add(1);

    if value == game.secret {
        return Outcome::Won {
            guesses: game.guesses,
            reward: reward_for(game.guesses),
        };
    }
    if game.guesses >= MAX_GUESSES {
        return Outcome::Lost {
            secret: game.secret,
        };
    }
    if value > game.secret {
        Outcome::Lower
    } else {
        Outcome::Higher
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_always_lands_inside_the_playable_range() {
        // Walk the whole byte space coarsely; nothing may escape MIN..=MAX.
        for n in (0u32..=u32::MAX).step_by(7_919_311) {
            let s = secret_from_bytes(n.to_le_bytes());
            assert!((MIN..=MAX).contains(&s), "{n} produced {s}");
        }
    }

    #[test]
    fn a_wild_secret_is_clamped_not_trusted() {
        assert_eq!(Game::new(9999, 0).secret, MAX);
        assert_eq!(Game::new(0, 0).secret, MIN);
    }

    #[test]
    fn direction_hints_point_the_right_way() {
        let mut g = Game::new(12, 0);
        assert_eq!(guess(&mut g, 5), Outcome::Higher, "5 < 12, aim higher");
        assert_eq!(guess(&mut g, 18), Outcome::Lower, "18 > 12, aim lower");
    }

    #[test]
    fn winning_first_try_pays_the_most() {
        let mut g = Game::new(7, 0);
        match guess(&mut g, 7) {
            Outcome::Won { guesses, reward } => {
                assert_eq!(guesses, 1);
                assert_eq!(reward, 30);
            }
            other => panic!("expected a win, got {other:?}"),
        }
    }

    #[test]
    fn reward_shrinks_with_each_guess_but_never_to_nothing() {
        let mut last = u8::MAX;
        for n in 1..=MAX_GUESSES {
            let r = reward_for(n);
            assert!(r <= last, "reward must not grow at guess {n}");
            assert!(r >= 10, "finishing must always be worth something");
            last = r;
        }
    }

    #[test]
    fn running_out_of_guesses_ends_the_round_and_reveals_the_secret() {
        let mut g = Game::new(20, 0);
        for _ in 1..MAX_GUESSES {
            assert!(matches!(guess(&mut g, 1), Outcome::Higher));
        }
        assert_eq!(guess(&mut g, 1), Outcome::Lost { secret: 20 });
    }

    #[test]
    fn a_last_chance_guess_can_still_win() {
        let mut g = Game::new(15, 0);
        for _ in 1..MAX_GUESSES {
            guess(&mut g, 1);
        }
        assert!(
            matches!(guess(&mut g, 15), Outcome::Won { .. }),
            "the final attempt must be winnable, not auto-lost"
        );
    }

    #[test]
    fn out_of_range_input_does_not_burn_an_attempt() {
        let mut g = Game::new(10, 0);
        assert_eq!(guess(&mut g, 0), Outcome::OutOfRange);
        assert_eq!(guess(&mut g, 99), Outcome::OutOfRange);
        assert_eq!(g.guesses, 0, "a typo must not cost the player a turn");
        assert_eq!(g.guesses_left(), MAX_GUESSES);
    }

    #[test]
    fn warmth_gets_hotter_as_you_close_in() {
        assert_eq!(warmth(10, 10), "spot on");
        assert_eq!(warmth(10, 11), "burning hot");
        assert_eq!(warmth(10, 14), "warm");
        assert_eq!(warmth(10, 17), "cool");
        assert_eq!(warmth(20, 1), "freezing");
    }

    #[test]
    fn guesses_left_counts_down_and_floors_at_zero() {
        let mut g = Game::new(5, 0);
        assert_eq!(g.guesses_left(), MAX_GUESSES);
        guess(&mut g, 1);
        assert_eq!(g.guesses_left(), MAX_GUESSES - 1);
        g.guesses = 99;
        assert_eq!(g.guesses_left(), 0, "must not underflow");
    }

    #[test]
    fn game_state_survives_a_json_round_trip() {
        let g = Game::new(13, 42);
        let s = serde_json::to_string(&g).unwrap();
        assert_eq!(serde_json::from_str::<Game>(&s).unwrap(), g);
    }
}
