//! The Home feeds already seen, one for each mood chip, so that going back to a mood
//! shows its feed at once instead of an empty page while the service answers again.

use std::collections::HashMap;

use crate::model::{Chip, Shelf};

/// One mood's feed as it last came.
#[derive(Clone, Debug, PartialEq)]
pub struct Feed {
    pub chips: Vec<Chip>,
    pub shelves: Vec<Shelf>,
    pub continuation: Option<String>,
}

/// The feeds seen since the app started (or since the account changed).
#[derive(Default)]
pub struct Feeds {
    by_mood: HashMap<String, Feed>,
}

/// Which mood these chips say is chosen: its title, or "" for none ("All").
pub fn mood(chips: &[Chip]) -> String {
    chips.iter().find(|c| c.selected).map(|c| c.title.clone()).unwrap_or_default()
}

/// Mark the chip that asks for `params` as chosen, at once, before the service answers
/// (it answers with its own marks). `None` is "All": no chip is chosen. Returns the mood.
pub fn choose(chips: &mut [Chip], params: &Option<String>) -> String {
    for c in chips.iter_mut() {
        c.selected = params.is_some() && c.params == *params;
    }
    mood(chips)
}

impl Feeds {
    /// Keep a feed that just came, under the mood its own chips name.
    pub fn keep(&mut self, feed: Feed) {
        if !feed.shelves.is_empty() {
            self.by_mood.insert(mood(&feed.chips), feed);
        }
    }

    pub fn of(&self, mood: &str) -> Option<&Feed> {
        self.by_mood.get(mood)
    }

    pub fn clear(&mut self) {
        self.by_mood.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ShelfKind;

    fn chip(title: &str, selected: bool) -> Chip {
        Chip { title: title.to_owned(), params: Some(format!("p-{title}")), selected }
    }

    fn shelf(title: &str) -> Shelf {
        Shelf { title: title.to_owned(), strapline: None, strap_thumb: None, kind: ShelfKind::Cards(Vec::new()) }
    }

    #[test]
    fn the_tapped_chip_is_chosen_before_the_service_answers() {
        let mut chips = vec![chip("Relax", false), chip("Sad", true)];
        assert_eq!(choose(&mut chips, &Some("p-Relax".to_owned())), "Relax");
        assert!(chips[0].selected && !chips[1].selected);
        assert_eq!(choose(&mut chips, &None), "", "All: no chip is chosen");
        assert!(chips.iter().all(|c| !c.selected));
    }

    #[test]
    fn a_mood_seen_before_has_its_feed_at_hand() {
        let mut feeds = Feeds::default();
        feeds.keep(Feed { chips: vec![chip("Relax", false), chip("Sad", false)], shelves: vec![shelf("Quick picks")], continuation: None });
        feeds.keep(Feed { chips: vec![chip("Relax", true), chip("Sad", false)], shelves: vec![shelf("Calm")], continuation: Some("more".to_owned()) });
        assert_eq!(feeds.of("").unwrap().shelves[0].title, "Quick picks", "All");
        assert_eq!(feeds.of("Relax").unwrap().shelves[0].title, "Calm");
        assert!(feeds.of("Sad").is_none(), "not seen yet");
        feeds.keep(Feed { chips: vec![chip("Relax", true)], shelves: vec![shelf("Calmer")], continuation: None });
        assert_eq!(feeds.of("Relax").unwrap().shelves[0].title, "Calmer", "the newer answer replaces the older");
    }

    #[test]
    fn an_empty_answer_is_not_kept_and_another_account_starts_afresh() {
        let mut feeds = Feeds::default();
        feeds.keep(Feed { chips: vec![chip("Relax", true)], shelves: Vec::new(), continuation: None });
        assert!(feeds.of("Relax").is_none());
        feeds.keep(Feed { chips: Vec::new(), shelves: vec![shelf("Quick picks")], continuation: None });
        feeds.clear();
        assert!(feeds.of("").is_none());
    }
}
