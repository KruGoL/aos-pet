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
    let low = f64::from(LOW);
    let high = f64::from(HIGH);
    let peak = f64::from(PEAK);
    if pet.sleeping {
        return Mood::Sleeping;
    }
    if pet.sick {
        return Mood::Sick;
    }
    if pet.fullness < low {
        return Mood::Hungry;
    }
    if pet.energy < low {
        return Mood::Tired;
    }
    // Dirty *and* delighted reads as a good afternoon, not neglect.
    if pet.cleanliness < low && pet.happiness >= high {
        return Mood::Scruffy;
    }
    if pet.cleanliness < low {
        return Mood::Dirty;
    }
    // Every other need met, yet unhappy — that is not a bar, that is you.
    if pet.happiness < low
        && pet.fullness >= high
        && pet.energy >= high
        && pet.cleanliness >= high
    {
        return Mood::Lonely;
    }
    if pet.happiness < low {
        return Mood::Sad;
    }
    if pet.fullness >= peak
        && pet.happiness >= peak
        && pet.energy >= peak
        && pet.cleanliness >= peak
    {
        return Mood::Radiant;
    }
    if pet.fullness >= high
        && pet.happiness >= high
        && pet.energy >= high
        && pet.cleanliness >= high
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
        p.fullness = 80.0;
        p.happiness = 80.0;
        p.energy = 80.0;
        p.cleanliness = 80.0;
        assert_eq!(mood_of(&p), Mood::Happy, "comfortably above HIGH");

        // Everything brimming is a step beyond merely happy.
        p.fullness = 100.0;
        p.happiness = 100.0;
        p.energy = 100.0;
        p.cleanliness = 100.0;
        assert_eq!(mood_of(&p), Mood::Radiant);
    }

    #[test]
    fn sleeping_outranks_every_other_condition() {
        let mut p = pet();
        p.sleeping = true;
        p.sick = true;
        p.fullness = 0.0;
        assert_eq!(mood_of(&p), Mood::Sleeping);
    }

    #[test]
    fn sickness_outranks_plain_needs() {
        let mut p = pet();
        p.sick = true;
        p.fullness = 0.0;
        assert_eq!(mood_of(&p), Mood::Sick);
    }

    #[test]
    fn needs_are_reported_in_urgency_order() {
        let mut p = pet();
        p.fullness = 10.0;
        p.energy = 10.0;
        assert_eq!(mood_of(&p), Mood::Hungry, "hunger beats tiredness");

        p.fullness = 90.0;
        assert_eq!(mood_of(&p), Mood::Tired);
    }

    #[test]
    fn the_low_threshold_is_exclusive() {
        let mut p = pet();
        p.fullness = f64::from(LOW);
        assert_ne!(mood_of(&p), Mood::Hungry, "exactly LOW is not yet hungry");
        p.fullness = f64::from(LOW) - 1.0;
        assert_eq!(mood_of(&p), Mood::Hungry);
    }

    #[test]
    fn a_pet_fed_and_washed_but_never_played_with_is_lonely_not_merely_sad() {
        let mut p = pet();
        p.fullness = 90.0;
        p.energy = 90.0;
        p.cleanliness = 90.0;
        p.happiness = 10.0;
        assert_eq!(mood_of(&p), Mood::Lonely);

        // Once something else is also lacking, it is ordinary unhappiness.
        p.energy = 50.0;
        assert_eq!(mood_of(&p), Mood::Sad);
    }

    #[test]
    fn dirty_but_delighted_is_scruffy_rather_than_a_reproach() {
        let mut p = pet();
        p.cleanliness = 5.0;
        p.happiness = 95.0;
        assert_eq!(mood_of(&p), Mood::Scruffy);

        p.happiness = 50.0;
        assert_eq!(mood_of(&p), Mood::Dirty, "without the joy it is just dirt");
    }

    #[test]
    fn radiant_needs_everything_brimming_not_merely_good() {
        let mut p = pet();
        p.fullness = f64::from(HIGH) + 1.0;
        p.happiness = f64::from(HIGH) + 1.0;
        p.energy = f64::from(HIGH) + 1.0;
        p.cleanliness = f64::from(HIGH) + 1.0;
        assert_eq!(mood_of(&p), Mood::Happy);

        p.fullness = f64::from(PEAK);
        p.happiness = f64::from(PEAK);
        p.energy = f64::from(PEAK);
        p.cleanliness = f64::from(PEAK);
        assert_eq!(mood_of(&p), Mood::Radiant);
    }

    #[test]
    fn middling_stats_are_neutral() {
        let mut p = pet();
        p.fullness = 50.0;
        p.happiness = 50.0;
        p.energy = 50.0;
        p.cleanliness = 50.0;
        assert_eq!(mood_of(&p), Mood::Neutral);
    }
}
