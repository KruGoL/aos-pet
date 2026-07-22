//! Deriving a single mood from the stats. Drives which art frame is shown.

use crate::config::{HIGH, LOW};
use crate::model::Pet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Sleeping,
    Sick,
    Hungry,
    Tired,
    Dirty,
    Sad,
    Happy,
    Neutral,
}

impl Mood {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sleeping => "sleeping",
            Self::Sick => "sick",
            Self::Hungry => "hungry",
            Self::Tired => "tired",
            Self::Dirty => "dirty",
            Self::Sad => "sad",
            Self::Happy => "happy",
            Self::Neutral => "content",
        }
    }
}

/// Priority order matters: the most urgent condition wins so the player is
/// shown the thing that needs attention first.
#[must_use]
pub fn mood_of(pet: &Pet) -> Mood {
    if pet.sleeping {
        return Mood::Sleeping;
    }
    if pet.sick {
        return Mood::Sick;
    }
    if pet.fullness < LOW {
        return Mood::Hungry;
    }
    if pet.energy < LOW {
        return Mood::Tired;
    }
    if pet.cleanliness < LOW {
        return Mood::Dirty;
    }
    if pet.happiness < LOW {
        return Mood::Sad;
    }
    if pet.fullness >= HIGH
        && pet.happiness >= HIGH
        && pet.energy >= HIGH
        && pet.cleanliness >= HIGH
    {
        return Mood::Happy;
    }
    Mood::Neutral
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pet() -> Pet {
        Pet::new("Rex".into(), 0)
    }

    #[test]
    fn a_well_kept_pet_is_happy() {
        let mut p = pet();
        p.fullness = 100;
        p.happiness = 100;
        p.energy = 100;
        p.cleanliness = 100;
        assert_eq!(mood_of(&p), Mood::Happy);
    }

    #[test]
    fn sleeping_outranks_every_other_condition() {
        let mut p = pet();
        p.sleeping = true;
        p.sick = true;
        p.fullness = 0;
        assert_eq!(mood_of(&p), Mood::Sleeping);
    }

    #[test]
    fn sickness_outranks_plain_needs() {
        let mut p = pet();
        p.sick = true;
        p.fullness = 0;
        assert_eq!(mood_of(&p), Mood::Sick);
    }

    #[test]
    fn needs_are_reported_in_urgency_order() {
        let mut p = pet();
        p.fullness = 10;
        p.energy = 10;
        assert_eq!(mood_of(&p), Mood::Hungry, "hunger beats tiredness");

        p.fullness = 90;
        assert_eq!(mood_of(&p), Mood::Tired);
    }

    #[test]
    fn the_low_threshold_is_exclusive() {
        let mut p = pet();
        p.fullness = LOW;
        assert_ne!(mood_of(&p), Mood::Hungry, "exactly LOW is not yet hungry");
        p.fullness = LOW - 1;
        assert_eq!(mood_of(&p), Mood::Hungry);
    }

    #[test]
    fn middling_stats_are_neutral() {
        let mut p = pet();
        p.fullness = 50;
        p.happiness = 50;
        p.energy = 50;
        p.cleanliness = 50;
        assert_eq!(mood_of(&p), Mood::Neutral);
    }
}
