//! ASCII art. Two frames per mood so a polling viewer can animate by
//! alternating them; a single call just shows frame 0.

use crate::mood::Mood;

pub const FRAME_COUNT: usize = 2;

/// Wrap the standard silhouette around a face and an effect. Moments supply
/// their own face so the pet visibly changes while one is running, without
/// every moment having to redraw the whole body.
#[must_use]
pub fn compose(face: &str, fx: &str) -> String {
    format!("    /\\_/\\\n   {face}   {fx}\n    > _ <")
}

/// The two animation frames for a mood, in order.
#[must_use]
pub fn frames(mood: Mood) -> [&'static str; FRAME_COUNT] {
    match mood {
        Mood::Happy => [
            r"    /\_/\
   ( ^.^ )
    > ^ <",
            r"    /\_/\
   ( -.^ )
    > ^ <",
        ],
        Mood::Neutral => [
            r"    /\_/\
   ( o.o )
    > _ <",
            r"    /\_/\
   ( -.- )
    > _ <",
        ],
        Mood::Hungry => [
            r"    /\_/\
   ( O.O )   ~rumble~
    > u <",
            r"    /\_/\
   ( o.O )   ~rumble~
    > u <",
        ],
        Mood::Tired => [
            r"    /\_/\
   ( -.- )   ~yawn~
    > _ <",
            r"    /\_/\
   ( u.u )
    > _ <",
        ],
        Mood::Dirty => [
            r"    /\_/\
   ( x.o )   *dust*
    > _ <   ' '",
            r"    /\_/\
   ( o.x )   *dust*
    > _ <  '  '",
        ],
        Mood::Scruffy => [
            r"    /\_/\
   ( ^.^ )   *poof*
    > u <   ' '",
            r"    /\_/\
   ( ^.- )   *poof*
    > u <  '  '",
        ],
        Mood::Lonely => [
            r"    /\_/\
   ( o.o )
    > _ <",
            r"    /\_/\
   ( .   )      <- looks away
    > _ <",
        ],
        Mood::Radiant => [
            r"    /\_/\    \ | /
   ( ^o^ )   --  --
    > ^ <    / | \",
            r"    /\_/\    / | \
   ( ^_^ )   --  --
    > ^ <    \ | /",
        ],
        Mood::Sad => [
            r"    /\_/\
   ( ;.; )
    > _ <",
            r"    /\_/\
   ( T.T )
    > _ <",
        ],
        Mood::Sick => [
            r"    /\_/\
   ( x.x )   +
    > ~ <",
            r"    /\_/\
   ( @.@ )   +
    > ~ <",
        ],
        Mood::Sleeping => [
            r"    /\_/\
   ( u.u )   z
    > _ <",
            r"    /\_/\
   ( u.u )     Z
    > _ <",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Mood; 11] = [
        Mood::Happy,
        Mood::Neutral,
        Mood::Hungry,
        Mood::Tired,
        Mood::Scruffy,
        Mood::Dirty,
        Mood::Lonely,
        Mood::Sad,
        Mood::Radiant,
        Mood::Sick,
        Mood::Sleeping,
    ];

    #[test]
    fn every_mood_has_two_non_empty_frames() {
        for m in ALL {
            for (i, f) in frames(m).iter().enumerate() {
                assert!(!f.trim().is_empty(), "{m:?} frame {i} is empty");
            }
        }
    }

    #[test]
    fn frames_differ_so_animation_is_visible() {
        for m in ALL {
            let [a, b] = frames(m);
            assert_ne!(a, b, "{m:?} frames are identical — nothing would animate");
        }
    }

    #[test]
    fn art_is_plain_ascii_so_every_terminal_can_render_it() {
        for m in ALL {
            for f in frames(m) {
                assert!(f.is_ascii(), "{m:?} art must stay ASCII");
            }
        }
    }
}
