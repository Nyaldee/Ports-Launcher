
use super::dialogs::{apply_theme, preview_colors};
use super::state::AppState;
use crate::core::konami::{KonamiDetector, KonamiInput};
use crate::ui::theme::ThemeColors;
use slint::platform::Key;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant, SystemTime};

const TICK: Duration = Duration::from_millis(20);
const COLOR_STEP: Duration = Duration::from_millis(120);
const FIRST_JUMP: Duration = Duration::from_millis(50);
const LAST_JUMP: Duration = Duration::from_millis(450);
const SLOWDOWN: f64 = 1.12;
const COLORS_ONLY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Konami {
    detector: RefCell<KonamiDetector>,
    running: Cell<bool>,
    timer: slint::Timer,
}

pub(crate) fn keyboard_input(text: &str, modified: bool) -> KonamiInput {
    let mut chars = text.chars();
    let (Some(c), None, false) = (chars.next(), chars.next(), modified) else { return KonamiInput::Other };
    match c {
        'a' | 'A' => KonamiInput::A,
        'b' | 'B' => KonamiInput::B,
        c if c == char::from(Key::UpArrow) => KonamiInput::Up,
        c if c == char::from(Key::DownArrow) => KonamiInput::Down,
        c if c == char::from(Key::LeftArrow) => KonamiInput::Left,
        c if c == char::from(Key::RightArrow) => KonamiInput::Right,
        c if c == char::from(Key::Return) => KonamiInput::A,
        _ => KonamiInput::Other,
    }
}

pub(crate) fn intercept(app: &Rc<AppState>, input: Option<KonamiInput>) -> bool {
    let konami = &app.konami;
    if konami.running.get() {
        return true;
    }
    let Some(input) = input else { return false };
    let expected = konami.detector.borrow().expects(input);
    if konami.detector.borrow_mut().feed(input) {
        start(app);
        return true;
    }
    expected && input == KonamiInput::B
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

fn start(app: &Rc<AppState>) {
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
    app.konami.running.set(true);
    let weak: Weak<AppState> = Rc::downgrade(app);
    app.konami.timer.start(slint::TimerMode::Repeated, TICK, move || {
        let Some(app) = weak.upgrade() else { return };
        if !roulette.tick(&app) {
            apply_theme(&app.window(), &app.theme.theme_config.borrow(), app.window_geometry.border_width.get());
            app.konami.running.set(false);
            app.konami.timer.stop();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clavier_fleches_b_a_et_entree() {
        assert_eq!(keyboard_input(&char::from(Key::UpArrow).to_string(), false), KonamiInput::Up);
        assert_eq!(keyboard_input("b", false), KonamiInput::B);
        assert_eq!(keyboard_input("A", false), KonamiInput::A);
        assert_eq!(keyboard_input(&char::from(Key::Return).to_string(), false), KonamiInput::A);
        assert_eq!(keyboard_input("a", true), KonamiInput::Other);
        assert_eq!(keyboard_input("x", false), KonamiInput::Other);
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
