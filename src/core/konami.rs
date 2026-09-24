
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KonamiInput {
    Up,
    Down,
    Left,
    Right,
    B,
    A,
    Other,
}

use KonamiInput::{Down, Left, Right, Up, A, B};

const SEQUENCE: [KonamiInput; 10] = [Up, Up, Down, Down, Left, Right, Left, Right, B, A];

#[derive(Default)]
pub struct KonamiDetector {
    history: Vec<KonamiInput>,
}

impl KonamiDetector {
    pub fn progress(&self) -> usize {
        (0..=self.history.len()).rev().find(|&k| self.history.ends_with(&SEQUENCE[..k])).unwrap_or(0)
    }

    pub fn expects(&self, input: KonamiInput) -> bool {
        SEQUENCE.get(self.progress()) == Some(&input)
    }

    pub fn feed(&mut self, input: KonamiInput) -> bool {
        if self.history.len() == SEQUENCE.len() {
            self.history.remove(0);
        }
        self.history.push(input);
        let complete = self.progress() == SEQUENCE.len();
        if complete {
            self.history.clear();
        }
        complete
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(detector: &mut KonamiDetector, inputs: &[KonamiInput]) -> bool {
        inputs.iter().fold(false, |_, &input| detector.feed(input))
    }

    #[test]
    fn sequence_complete_declenche_une_seule_fois() {
        let mut detector = KonamiDetector::default();
        assert!(feed_all(&mut detector, &SEQUENCE));
        assert_eq!(detector.progress(), 0);
        assert!(!detector.feed(A));
    }

    #[test]
    fn haut_supplementaire_ne_casse_pas_la_sequence() {
        let mut detector = KonamiDetector::default();
        assert!(feed_all(&mut detector, &[Up, Up, Up, Down, Down, Left, Right, Left, Right, B, A]));
    }

    #[test]
    fn erreur_puis_reprise() {
        let mut detector = KonamiDetector::default();
        feed_all(&mut detector, &[Up, Up, Down, KonamiInput::Other]);
        assert_eq!(detector.progress(), 0);
        assert!(feed_all(&mut detector, &SEQUENCE));
    }

    #[test]
    fn progression_suit_la_saisie() {
        let mut detector = KonamiDetector::default();
        feed_all(&mut detector, &SEQUENCE[..8]);
        assert!(detector.expects(B));
        detector.feed(B);
        assert_eq!(detector.progress(), 9);
        assert!(detector.expects(A));
        detector.feed(Up);
        assert_eq!(detector.progress(), 1);
    }
}
