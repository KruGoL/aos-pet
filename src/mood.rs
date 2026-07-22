//! Deriving a single mood from the stats. Drives which art frame is shown.

use crate::config::{HIGH, LOW, PEAK};
use crate::model::Pet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Sleeping,
    Sick,
    Hungry,
    Tired,
    /// Filthy and thrilled about it — the first low bar that is not a reproach.
    Scruffy,
    Dirty,
    /// Every need met and still miserable: you feed it and wash it, but never
    /// play with it. The only mood that reads the carer rather than the pet.
    Lonely,
    Sad,
    /// Not merely fine — everything brimming at once.
    Radiant,
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
            Self::Scruffy => "scruffy",
            Self::Dirty => "dirty",
            Self::Lonely => "lonely",
            Self::Sad => "sad",
            Self::Radiant => "radiant",
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
    // Dirty *and* delighted reads as a good afternoon, not neglect.
    if pet.cleanliness < LOW && pet.happiness >= HIGH {
        return Mood::Scruffy;
    }
    if pet.cleanliness < LOW {
        return Mood::Dirty;
    }
    // Every other need met, yet unhappy — that is not a bar, that is you.
    if pet.happiness < LOW
        && pet.fullness >= HIGH
        && pet.energy >= HIGH
        && pet.cleanliness >= HIGH
    {
        return Mood::Lonely;
    }
    if pet.happiness < LOW {
        return Mood::Sad;
    }
    if pet.fullness >= PEAK
        && pet.happiness >= PEAK
        && pet.energy >= PEAK
        && pet.cleanliness >= PEAK
    {
        return Mood::Radiant;
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
        p.fullness = 80;
        p.happiness = 80;
        p.energy = 80;
        p.cleanliness = 80;
        assert_eq!(mood_of(&p), Mood::Happy, "comfortably above HIGH");

        // Everything brimming is a step beyond merely happy.
        p.fullness = 100;
        p.happiness = 100;
        p.energy = 100;
        p.cleanliness = 100;
        assert_eq!(mood_of(&p), Mood::Radiant);
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
    fn a_pet_fed_and_washed_but_never_played_with_is_lonely_not_merely_sad() {
        let mut p = pet();
        p.fullness = 90;
        p.energy = 90;
        p.cleanliness = 90;
        p.happiness = 10;
        assert_eq!(mood_of(&p), Mood::Lonely);

        // Once something else is also lacking, it is ordinary unhappiness.
        p.energy = 50;
        assert_eq!(mood_of(&p), Mood::Sad);
    }

    #[test]
    fn dirty_but_delighted_is_scruffy_rather_than_a_reproach() {
        let mut p = pet();
        p.cleanliness = 5;
        p.happiness = 95;
        assert_eq!(mood_of(&p), Mood::Scruffy);

        p.happiness = 50;
        assert_eq!(mood_of(&p), Mood::Dirty, "without the joy it is just dirt");
    }

    #[test]
    fn radiant_needs_everything_brimming_not_merely_good() {
        let mut p = pet();
        p.fullness = HIGH + 1;
        p.happiness = HIGH + 1;
        p.energy = HIGH + 1;
        p.cleanliness = HIGH + 1;
        assert_eq!(mood_of(&p), Mood::Happy);

        p.fullness = PEAK;
        p.happiness = PEAK;
        p.energy = PEAK;
        p.cleanliness = PEAK;
        assert_eq!(mood_of(&p), Mood::Radiant);
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
