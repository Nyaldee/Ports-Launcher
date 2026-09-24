
use crate::core::gamepad::{GamepadPoller, GamepadState};
use crate::core::konami::KonamiInput;
use std::rc::Rc;
use std::time::{Duration, Instant};

const STICK_DEADZONE: f32 = 8000.0 / 32767.0;
const NAV_REPEAT_DELAY: Duration = Duration::from_millis(350);
pub const POLL_INTERVAL_MS: u64 = 20;

const DIRECTION_DELTAS: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];
const DIRECTION_INPUTS: [KonamiInput; 4] = [KonamiInput::Up, KonamiInput::Down, KonamiInput::Left, KonamiInput::Right];

pub trait GamepadTarget {
    fn intercept(&self, _input: Option<KonamiInput>) -> bool {
        false
    }
    fn move_selection(&self, _dx: i32, _dy: i32) {}
    fn activate_selection(&self) {}
    fn reject(&self) {}
    fn show_info_for_selection(&self) {}
    fn toggle_fullscreen(&self) {}
    fn open_settings(&self) {}
}

fn read_directions(state: &GamepadState) -> [bool; 4] {
    [
        state.dpad_up || state.stick_y > STICK_DEADZONE,
        state.dpad_down || state.stick_y < -STICK_DEADZONE,
        state.dpad_left || state.stick_x < -STICK_DEADZONE,
        state.dpad_right || state.stick_x > STICK_DEADZONE,
    ]
}

fn read_buttons(state: &GamepadState) -> [bool; BUTTON_COUNT] {
    [state.button_a, state.button_start, state.button_b, state.button_x, state.button_back, state.button_y]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAction {
    ActivateSelection,
    Reject,
    ShowInfoForSelection,
    ToggleFullscreen,
    OpenSettings,
}

const BUTTON_COUNT: usize = 6;

const BUTTON_ACTIONS: [GamepadAction; BUTTON_COUNT] = [
    GamepadAction::ActivateSelection,
    GamepadAction::ActivateSelection,
    GamepadAction::Reject,
    GamepadAction::ShowInfoForSelection,
    GamepadAction::ToggleFullscreen,
    GamepadAction::OpenSettings,
];

const BUTTON_INPUTS: [KonamiInput; BUTTON_COUNT] =
    [KonamiInput::A, KonamiInput::Other, KonamiInput::B, KonamiInput::Other, KonamiInput::Other, KonamiInput::Other];

type Move = ((i32, i32), Option<KonamiInput>);

fn decide_moves(prev: [bool; 4], current: [bool; 4], last_move: &mut [Instant; 4], repeating: &mut [bool; 4], now: Instant) -> Vec<Move> {
    let mut moves = Vec::new();
    for i in 0..4 {
        if !current[i] {
            repeating[i] = false;
            continue;
        }
        let fresh_press = !prev[i];
        if fresh_press || repeating[i] || now.duration_since(last_move[i]) > NAV_REPEAT_DELAY {
            moves.push((DIRECTION_DELTAS[i], fresh_press.then_some(DIRECTION_INPUTS[i])));
            last_move[i] = now;
            repeating[i] = !fresh_press;
        }
    }
    moves
}

fn decide_button_actions(prev: [bool; BUTTON_COUNT], current: [bool; BUTTON_COUNT]) -> Vec<(GamepadAction, KonamiInput)> {
    (0..BUTTON_COUNT).filter(|&i| current[i] && !prev[i]).map(|i| (BUTTON_ACTIONS[i], BUTTON_INPUTS[i])).collect()
}

pub struct GamepadRouter {
    poller: GamepadPoller,
    stack: Vec<Rc<dyn GamepadTarget>>,
    held_dirs: [bool; 4],
    held_buttons: [bool; BUTTON_COUNT],
    last_move: [Instant; 4],
    repeating: [bool; 4],
}

impl GamepadRouter {
    pub fn new() -> GamepadRouter {
        let now = Instant::now();
        GamepadRouter {
            poller: GamepadPoller::new(),
            stack: Vec::new(),
            held_dirs: [false; 4],
            held_buttons: [false; BUTTON_COUNT],
            last_move: [now; 4],
            repeating: [false; 4],
        }
    }

    pub fn is_available(&self) -> bool {
        self.poller.is_available()
    }

    pub fn push_target(&mut self, target: Rc<dyn GamepadTarget>) {
        self.stack.push(target);
        self.reseed();
    }

    pub fn pop_target(&mut self) {
        self.stack.pop();
        self.reseed();
    }

    fn reseed(&mut self) {
        let state = self.poller.poll().unwrap_or_default();
        self.held_dirs = read_directions(&state);
        self.held_buttons = read_buttons(&state);
    }

    pub fn poll(&mut self) -> Option<PollResult> {
        let target = self.stack.last()?.clone();
        let state = self.poller.poll()?;

        let directions = read_directions(&state);
        let moves = decide_moves(self.held_dirs, directions, &mut self.last_move, &mut self.repeating, Instant::now());
        self.held_dirs = directions;

        let buttons = read_buttons(&state);
        let actions = decide_button_actions(self.held_buttons, buttons);
        self.held_buttons = buttons;

        Some(PollResult { target, moves, actions })
    }
}

impl Default for GamepadRouter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PollResult {
    target: Rc<dyn GamepadTarget>,
    moves: Vec<Move>,
    actions: Vec<(GamepadAction, KonamiInput)>,
}

pub fn dispatch(result: PollResult) {
    for ((dx, dy), input) in result.moves {
        if !result.target.intercept(input) {
            result.target.move_selection(dx, dy);
        }
    }
    for (action, input) in result.actions {
        if result.target.intercept(Some(input)) {
            continue;
        }
        match action {
            GamepadAction::ActivateSelection => result.target.activate_selection(),
            GamepadAction::Reject => result.target.reject(),
            GamepadAction::ShowInfoForSelection => result.target.show_info_for_selection(),
            GamepadAction::ToggleFullscreen => result.target.toggle_fullscreen(),
            GamepadAction::OpenSettings => result.target.open_settings(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: [bool; 4] = [true, false, false, false];

    #[test]
    fn decide_moves_premier_appui_declenche_immediatement() {
        let now = Instant::now();
        let mut last_move = [now; 4];
        let mut repeating = [false; 4];
        assert_eq!(decide_moves([false; 4], UP, &mut last_move, &mut repeating, now), vec![((0, -1), Some(KonamiInput::Up))]);
    }

    #[test]
    fn decide_moves_pas_de_repetition_avant_le_delai() {
        let now = Instant::now();
        let mut last_move = [now; 4];
        let mut repeating = [false; 4];
        assert!(decide_moves(UP, UP, &mut last_move, &mut repeating, now).is_empty());
    }

    #[test]
    fn decide_moves_repete_apres_le_delai_puis_a_chaque_scrutation() {
        let now = Instant::now();
        let mut last_move = [now - NAV_REPEAT_DELAY * 2; 4];
        let mut repeating = [false; 4];
        assert_eq!(decide_moves(UP, UP, &mut last_move, &mut repeating, now), vec![((0, -1), None)]);
        assert!(repeating[0]);
        let later = now + Duration::from_millis(1);
        assert_eq!(decide_moves(UP, UP, &mut last_move, &mut repeating, later), vec![((0, -1), None)]);
    }

    #[test]
    fn decide_moves_relachement_puis_reappui_redeclenche() {
        let now = Instant::now();
        let mut last_move = [now; 4];
        let mut repeating = [true; 4];
        assert!(decide_moves(UP, [false; 4], &mut last_move, &mut repeating, now).is_empty());
        assert!(!repeating[0]);
        assert_eq!(decide_moves([false; 4], UP, &mut last_move, &mut repeating, now), vec![((0, -1), Some(KonamiInput::Up))]);
    }

    #[test]
    fn decide_button_actions_front_montant_uniquement() {
        let a_held = [true, false, false, false, false, false];
        assert!(decide_button_actions(a_held, a_held).is_empty());
        assert_eq!(decide_button_actions([false; BUTTON_COUNT], a_held), vec![(GamepadAction::ActivateSelection, KonamiInput::A)]);
    }

    #[test]
    fn decide_button_actions_couvre_tous_les_boutons() {
        let pressed = |i: usize| {
            let mut buttons = [false; BUTTON_COUNT];
            buttons[i] = true;
            decide_button_actions([false; BUTTON_COUNT], buttons).into_iter().map(|(action, _)| action).collect::<Vec<_>>()
        };
        assert_eq!(pressed(1), vec![GamepadAction::ActivateSelection]);
        assert_eq!(pressed(2), vec![GamepadAction::Reject]);
        assert_eq!(pressed(3), vec![GamepadAction::ShowInfoForSelection]);
        assert_eq!(pressed(4), vec![GamepadAction::ToggleFullscreen]);
        assert_eq!(pressed(5), vec![GamepadAction::OpenSettings]);
    }

    #[test]
    fn empiler_et_depiler_sans_manette_ne_panique_pas() {
        struct Noop;
        impl GamepadTarget for Noop {}
        let mut router = GamepadRouter::new();
        assert!(router.poll().is_none());
        router.push_target(Rc::new(Noop));
        router.push_target(Rc::new(Noop));
        router.pop_target();
        assert_eq!(router.stack.len(), 1);
        let _ = router.poll();
    }
}
