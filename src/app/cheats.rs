
use super::dialogs::{apply_theme, preview_colors};
use super::state::AppState;
use crate::ui::theme::ThemeColors;
use slint::platform::Key;
use slint::Color;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheatInput {
    Up,
    Down,
    Left,
    Right,
    B,
    A,
    Y,
    Other,
}

use CheatInput::{Down, Left, Right, Up, A, B, Y};

const KONAMI: [CheatInput; 10] = [Up, Up, Down, Down, Left, Right, Left, Right, B, A];
const FINAL_BOUT: [CheatInput; 22] = [Right, Left, Down, Up, Right, Left, Down, Up, Y, Y, Y, Y, Y, A, A, A, A, A, A, A, A, A];
const BARREL_ROLL: &str = "do a barrel roll";
const ASKEW: &str = "askew";
const BIG_HEAD_MODE: &str = "big head mode";
const POWER_OVERWHELMING: &str = "power overwhelming";
const IDDQD: &str = "iddqd";
const IDDQD_MESSAGE: &str = "Degreelessness Mode On";
const IDDQD_DURATION: Duration = Duration::from_secs(5);

const TICK: Duration = Duration::from_millis(20);
const COLOR_STEP: Duration = Duration::from_millis(120);
const FIRST_JUMP: Duration = Duration::from_millis(50);
const LAST_JUMP: Duration = Duration::from_millis(450);
const SLOWDOWN: f64 = 1.12;
const COLORS_ONLY: Duration = Duration::from_secs(2);

struct SequenceDetector {
    sequence: &'static [CheatInput],
    history: Vec<CheatInput>,
}

impl SequenceDetector {
    fn new(sequence: &'static [CheatInput]) -> SequenceDetector {
        SequenceDetector { sequence, history: Vec::new() }
    }

    fn progress(&self) -> usize {
        (0..=self.history.len()).rev().find(|&k| self.history.ends_with(&self.sequence[..k])).unwrap_or(0)
    }

    fn expects(&self, input: CheatInput) -> bool {
        self.sequence.get(self.progress()) == Some(&input)
    }

    fn feed(&mut self, input: CheatInput) -> bool {
        if self.history.len() == self.sequence.len() {
            self.history.remove(0);
        }
        self.history.push(input);
        let complete = self.progress() == self.sequence.len();
        if complete {
            self.history.clear();
        }
        complete
    }
}

pub(crate) struct Cheats {
    konami: RefCell<SequenceDetector>,
    final_bout: RefCell<SequenceDetector>,
    running: Cell<bool>,
    timer: slint::Timer,
    iddqd_timer: slint::Timer,
}

impl Default for Cheats {
    fn default() -> Cheats {
        Cheats {
            konami: RefCell::new(SequenceDetector::new(&KONAMI)),
            final_bout: RefCell::new(SequenceDetector::new(&FINAL_BOUT)),
            running: Cell::default(),
            timer: slint::Timer::default(),
            iddqd_timer: slint::Timer::default(),
        }
    }
}

pub(crate) fn keyboard_input(text: &str, modified: bool) -> CheatInput {
    let mut chars = text.chars();
    let (Some(c), None, false) = (chars.next(), chars.next(), modified) else { return CheatInput::Other };
    match c {
        'a' | 'A' => A,
        'b' | 'B' => B,
        'y' | 'Y' => Y,
        c if c == char::from(Key::UpArrow) => Up,
        c if c == char::from(Key::DownArrow) => Down,
        c if c == char::from(Key::LeftArrow) => Left,
        c if c == char::from(Key::RightArrow) => Right,
        c if c == char::from(Key::Return) => A,
        _ => CheatInput::Other,
    }
}

pub(crate) fn intercept(app: &Rc<AppState>, input: Option<CheatInput>) -> bool {
    let cheats = &app.cheats;
    if cheats.running.get() {
        return true;
    }
    let Some(input) = input else { return false };
    let is_button = matches!(input, A | B | Y);
    let expected = cheats.konami.borrow().expects(input) || cheats.final_bout.borrow().expects(input);
    let konami_done = cheats.konami.borrow_mut().feed(input);
    let final_bout_done = cheats.final_bout.borrow_mut().feed(input);
    if konami_done {
        start_konami(app);
    }
    if final_bout_done {
        app.window().invoke_shake();
    }
    konami_done || final_bout_done || (expected && is_button)
}

pub(crate) fn search_changed(app: &Rc<AppState>, query: &str) -> bool {
    let window = app.window();
    match query.trim().to_lowercase().as_str() {
        BARREL_ROLL => window.invoke_barrel_roll(),
        ASKEW => window.invoke_askew(),
        BIG_HEAD_MODE => window.invoke_big_head(),
        POWER_OVERWHELMING => window.invoke_shake(),
        IDDQD => start_iddqd(app),
        _ => return false,
    }
    window.invoke_clear_search();
    true
}

fn negative_grey(colors: &ThemeColors) -> ThemeColors {
    let invert = |c: Color| {
        let luma = 0.299 * f32::from(c.red()) + 0.587 * f32::from(c.green()) + 0.114 * f32::from(c.blue());
        let v = 255 - luma.round() as u8;
        Color::from_rgb_u8(v, v, v)
    };
    ThemeColors {
        search_background: invert(colors.search_background),
        search_text: invert(colors.search_text),
        list_background: invert(colors.list_background),
        list_text: invert(colors.list_text),
        selected_background: invert(colors.selected_background),
        selected_text: invert(colors.selected_text),
        border: invert(colors.border),
    }
}

fn start_iddqd(app: &Rc<AppState>) {
    let window = app.window();
    let cfg = app.theme.theme_config.borrow();
    preview_colors(&window, &cfg, &negative_grey(&cfg.current), app.window_geometry.border_width.get());
    window.set_placeholder_text(IDDQD_MESSAGE.into());
    let weak = Rc::downgrade(app);
    app.cheats.iddqd_timer.start(slint::TimerMode::SingleShot, IDDQD_DURATION, move || {
        let Some(app) = weak.upgrade() else { return };
        let window = app.window();
        apply_theme(&window, &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
        window.set_placeholder_text(app.state.borrow().placeholder_text.clone().into());
    });
}

struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Rng(nanos | 1)
    }

    fn pick(&mut self, len: usize, previous: Option<usize>) -> usize {
        loop {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            let index = (self.0 % len as u64) as usize;
            if len < 2 || Some(index) != previous {
                return index;
            }
        }
    }
}

struct Roulette {
    rng: Rng,
    big_mode: bool,
    candidates: Vec<usize>,
    colors: Vec<ThemeColors>,
    started: Instant,
    next_color: Instant,
    color_index: Option<usize>,
    next_jump: Instant,
    jump_delay: Duration,
    candidate_index: Option<usize>,
}

impl Roulette {
    fn tick(&mut self, app: &AppState) -> bool {
        if app.window().get_big_mode() != self.big_mode {
            return false;
        }
        let now = Instant::now();
        if now >= self.next_color && self.colors.len() > 1 {
            let index = self.rng.pick(self.colors.len(), self.color_index);
            self.color_index = Some(index);
            preview_colors(&app.window(), &app.theme.theme_config.borrow(), &self.colors[index], app.window_geometry.border_width.get());
            self.next_color = now + COLOR_STEP;
        }
        if self.candidates.is_empty() {
            return now.duration_since(self.started) < COLORS_ONLY;
        }
        if now < self.next_jump {
            return true;
        }
        let index = self.rng.pick(self.candidates.len(), self.candidate_index);
        self.candidate_index = Some(index);
        app.select_displayed(self.candidates[index]);
        if self.jump_delay >= LAST_JUMP {
            return false;
        }
        self.jump_delay = self.jump_delay.mul_f64(SLOWDOWN);
        self.next_jump = now + self.jump_delay;
        true
    }
}

fn start_konami(app: &Rc<AppState>) {
    let big_mode = app.window().get_big_mode();
    let candidates: Vec<usize> = if big_mode {
        (0..app.grid_nav.displayed_installed.borrow().len()).collect()
    } else {
        let displayed = app.windowed_nav.displayed_windowed.borrow();
        displayed.iter().enumerate().filter(|(_, port)| app.is_installed(port)).map(|(i, _)| i).collect()
    };
    let now = Instant::now();
    let mut roulette = Roulette {
        rng: Rng::new(),
        big_mode,
        candidates,
        colors: app.theme.theme_config.borrow().themes.values().copied().collect(),
        started: now,
        next_color: now,
        color_index: None,
        next_jump: now,
        jump_delay: FIRST_JUMP,
        candidate_index: None,
    };
    app.cheats.running.set(true);
    let weak: Weak<AppState> = Rc::downgrade(app);
    app.cheats.timer.start(slint::TimerMode::Repeated, TICK, move || {
        let Some(app) = weak.upgrade() else { return };
        if !roulette.tick(&app) {
            apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
            app.cheats.running.set(false);
            app.cheats.timer.stop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(detector: &mut SequenceDetector, inputs: &[CheatInput]) -> bool {
        inputs.iter().fold(false, |_, &input| detector.feed(input))
    }

    #[test]
    fn sequence_complete_declenche_une_seule_fois() {
        let mut detector = SequenceDetector::new(&KONAMI);
        assert!(feed_all(&mut detector, &KONAMI));
        assert_eq!(detector.progress(), 0);
        assert!(!detector.feed(A));
    }

    #[test]
    fn haut_supplementaire_ne_casse_pas_la_sequence() {
        let mut detector = SequenceDetector::new(&KONAMI);
        assert!(feed_all(&mut detector, &[Up, Up, Up, Down, Down, Left, Right, Left, Right, B, A]));
    }

    #[test]
    fn erreur_puis_reprise() {
        let mut detector = SequenceDetector::new(&KONAMI);
        feed_all(&mut detector, &[Up, Up, Down, CheatInput::Other]);
        assert_eq!(detector.progress(), 0);
        assert!(feed_all(&mut detector, &KONAMI));
    }

    #[test]
    fn progression_suit_la_saisie() {
        let mut detector = SequenceDetector::new(&KONAMI);
        feed_all(&mut detector, &KONAMI[..8]);
        assert!(detector.expects(B));
        detector.feed(B);
        assert_eq!(detector.progress(), 9);
        assert!(detector.expects(A));
        detector.feed(Up);
        assert_eq!(detector.progress(), 1);
    }

    #[test]
    fn final_bout_demande_le_bon_nombre_de_triangles_et_de_croix() {
        let mut detector = SequenceDetector::new(&FINAL_BOUT);
        assert!(!feed_all(&mut detector, &FINAL_BOUT[..21]));
        assert!(detector.expects(A));
        assert!(detector.feed(A));
        let mut detector = SequenceDetector::new(&FINAL_BOUT);
        feed_all(&mut detector, &FINAL_BOUT[..13]);
        assert!(!detector.expects(Y));
    }

    #[test]
    fn clavier_fleches_b_y_a_et_entree() {
        assert_eq!(keyboard_input(&char::from(Key::UpArrow).to_string(), false), Up);
        assert_eq!(keyboard_input("b", false), B);
        assert_eq!(keyboard_input("A", false), A);
        assert_eq!(keyboard_input("y", false), Y);
        assert_eq!(keyboard_input(&char::from(Key::Return).to_string(), false), A);
        assert_eq!(keyboard_input("a", true), CheatInput::Other);
        assert_eq!(keyboard_input("x", false), CheatInput::Other);
    }

    #[test]
    fn negatif_gris_inverse_la_luminosite() {
        let black = Color::from_rgb_u8(0, 0, 0);
        let white = Color::from_rgb_u8(255, 255, 255);
        let colors = ThemeColors {
            search_background: black,
            search_text: white,
            list_background: black,
            list_text: white,
            selected_background: Color::from_rgb_u8(255, 0, 0),
            selected_text: white,
            border: black,
        };
        let negative = negative_grey(&colors);
        assert_eq!(negative.list_background, white);
        assert_eq!(negative.list_text, black);
        assert_eq!(negative.selected_background, Color::from_rgb_u8(179, 179, 179));
    }

    #[test]
    fn pick_evite_la_valeur_precedente() {
        let mut rng = Rng::new();
        for _ in 0..100 {
            assert_ne!(rng.pick(3, Some(1)), 1);
        }
        assert_eq!(rng.pick(1, Some(0)), 0);
    }
}
