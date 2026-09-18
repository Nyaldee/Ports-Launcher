
use crate::core::gamepad::{GamepadPoller, GamepadState};
use std::rc::Rc;
use std::time::{Duration, Instant};

const STICK_DEADZONE: f32 = 8000.0 / 32767.0;
const NAV_REPEAT_DELAY: Duration = Duration::from_millis(350);
pub const POLL_INTERVAL_MS: u64 = 20;

const DIRECTION_DELTAS: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

pub trait GamepadTarget {
    fn move_selection(&self, _dx: i32, _dy: i32) {}
    fn activate_selection(&self) {}
    fn reject(&self) {}
    fn show_info_for_selection(&self) {}
    fn toggle_fullscreen(&self) {}
}

fn read_directions(state: &GamepadState) -> [bool; 4] {
    [
        state.dpad_up || state.stick_y > STICK_DEADZONE,
        state.dpad_down || state.stick_y < -STICK_DEADZONE,
        state.dpad_left || state.stick_x < -STICK_DEADZONE,
        state.dpad_right || state.stick_x > STICK_DEADZONE,
    ]
}

fn read_buttons(state: &GamepadState) -> [bool; 5] {
    [state.button_a, state.button_start, state.button_b, state.button_x, state.button_back]
}

fn decide_moves(
    prev: [bool; 4],
    current: [bool; 4],
    last_move: &mut [Instant; 4],
    repeating: &mut [bool; 4],
    now: Instant,
) -> Vec<(i32, i32)> {
    let mut moves = Vec::new();
    for i in 0..4 {
        if !current[i] {
            repeating[i] = false;
            continue;
        }
        if !prev[i] {
            moves.push(DIRECTION_DELTAS[i]);
            last_move[i] = now;
            repeating[i] = false;
        } else if repeating[i] || now.duration_since(last_move[i]) > NAV_REPEAT_DELAY {
            moves.push(DIRECTION_DELTAS[i]);
            last_move[i] = now;
            repeating[i] = true;
        }
    }
    moves
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAction {
    ActivateSelection,
    Reject,
    ShowInfoForSelection,
    ToggleFullscreen,
}

fn decide_button_actions(prev: [bool; 5], current: [bool; 5]) -> Vec<GamepadAction> {
    const ACTIONS: [GamepadAction; 5] = [
        GamepadAction::ActivateSelection,
        GamepadAction::ActivateSelection,
        GamepadAction::Reject,
        GamepadAction::ShowInfoForSelection,
        GamepadAction::ToggleFullscreen,
    ];
    let mut actions = Vec::new();
    for i in 0..5 {
        if current[i] && !prev[i] {
            actions.push(ACTIONS[i]);
        }
    }
    actions
}

pub struct GamepadRouter {
    poller: GamepadPoller,
    stack: Vec<Rc<dyn GamepadTarget>>,
    held_dirs: [bool; 4],
    held_buttons: [bool; 5],
    last_move: [Instant; 4],
    repeating: [bool; 4],
}

impl GamepadRouter {
    pub fn new() -> GamepadRouter {
        let epoch = Instant::now().checked_sub(NAV_REPEAT_DELAY * 2).unwrap_or_else(Instant::now);
        GamepadRouter {
            poller: GamepadPoller::new(),
            stack: Vec::new(),
            held_dirs: [false; 4],
            held_buttons: [false; 5],
            last_move: [epoch; 4],
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

    #[cfg(test)]
    fn active_dialog(&self) -> Option<&Rc<dyn GamepadTarget>> {
        if self.stack.len() > 1 {
            self.stack.last()
        } else {
            None
        }
    }

    fn reseed(&mut self) {
        let state = self.poller.poll().unwrap_or_default();
        self.held_dirs = read_directions(&state);
        self.held_buttons = read_buttons(&state);
    }

    pub fn poll(&mut self) -> Option<PollResult> {
        if self.stack.is_empty() {
            return None;
        }
        let state = self.poller.poll()?;
        let target = self.stack.last().unwrap().clone();
        let now = Instant::now();

        let directions = read_directions(&state);
        let moves = decide_moves(self.held_dirs, directions, &mut self.last_move, &mut self.repeating, now);
        self.held_dirs = directions;

        let buttons = read_buttons(&state);
        let actions = decide_button_actions(self.held_buttons, buttons);
        self.held_buttons = buttons;

        Some(PollResult { target, moves, actions })
    }
}

pub struct PollResult {
    pub target: Rc<dyn GamepadTarget>,
    pub moves: Vec<(i32, i32)>,
    pub actions: Vec<GamepadAction>,
}

pub fn dispatch(result: PollResult) {
    for (dx, dy) in result.moves {
        result.target.move_selection(dx, dy);
    }
    for action in result.actions {
        match action {
            GamepadAction::ActivateSelection => result.target.activate_selection(),
            GamepadAction::Reject => result.target.reject(),
            GamepadAction::ShowInfoForSelection => result.target.show_info_for_selection(),
            GamepadAction::ToggleFullscreen => result.target.toggle_fullscreen(),
        }
    }
}

impl Default for GamepadRouter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_moves_premier_appui_declenche_immediatement() {
        let mut last_move = [Instant::now() - NAV_REPEAT_DELAY * 2; 4];
        let mut repeating = [false; 4];
        let moves = decide_moves([false; 4], [true, false, false, false], &mut last_move, &mut repeating, Instant::now());
        assert_eq!(moves, vec![(0, -1)]);
    }

    #[test]
    fn decide_moves_pas_de_repetition_avant_le_delai() {
        let now = Instant::now();
        let mut last_move = [now; 4];
        let mut repeating = [false; 4];
        let moves = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert!(moves.is_empty());
    }

    #[test]
    fn decide_moves_repete_apres_le_delai() {
        let now = Instant::now();
        let mut last_move = [now - NAV_REPEAT_DELAY * 2; 4];
        let mut repeating = [false; 4];
        let moves = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert_eq!(moves, vec![(0, -1)]);
    }

    #[test]
    fn decide_moves_relachement_puis_reappui_redeclenche() {
        let now = Instant::now();
        let mut last_move = [now; 4];
        let mut repeating = [false; 4];
        let moves = decide_moves([false, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert_eq!(moves, vec![(0, -1)]);
    }

    #[test]
    fn decide_moves_repete_a_chaque_tick_une_fois_engagee() {
        let now = Instant::now();
        let mut last_move = [now - NAV_REPEAT_DELAY * 2; 4];
        let mut repeating = [false; 4];
        let moves = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert_eq!(moves, vec![(0, -1)]);
        assert!(repeating[0]);

        let now2 = now + Duration::from_millis(1);
        let moves2 = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now2);
        assert_eq!(moves2, vec![(0, -1)]);
    }

    #[test]
    fn decide_button_actions_front_montant_uniquement() {
        let actions = decide_button_actions([true, false, false, false, false], [true, false, false, false, false]);
        assert!(actions.is_empty());

        let actions = decide_button_actions([false, false, false, false, false], [true, false, false, false, false]);
        assert_eq!(actions, vec![GamepadAction::ActivateSelection]);
    }

    #[test]
    fn decide_button_actions_a_et_start_font_la_meme_action() {
        let actions = decide_button_actions([false, false, false, false, false], [false, true, false, false, false]);
        assert_eq!(actions, vec![GamepadAction::ActivateSelection]);
    }

    #[test]
    fn decide_button_actions_toutes_les_actions() {
        let actions = decide_button_actions([false; 5], [false, false, true, false, false]);
        assert_eq!(actions, vec![GamepadAction::Reject]);
        let actions = decide_button_actions([false; 5], [false, false, false, true, false]);
        assert_eq!(actions, vec![GamepadAction::ShowInfoForSelection]);
        let actions = decide_button_actions([false; 5], [false, false, false, false, true]);
        assert_eq!(actions, vec![GamepadAction::ToggleFullscreen]);
    }

    #[test]
    fn push_pop_target_reseed_sans_manette_ne_panique_pas() {
        struct Noop;
        impl GamepadTarget for Noop {}
        let mut router = GamepadRouter::new();
        router.push_target(Rc::new(Noop));
        assert!(router.active_dialog().is_none());
        router.push_target(Rc::new(Noop));
        assert!(router.active_dialog().is_some());
        router.pop_target();
        assert!(router.active_dialog().is_none());
        router.poll();
    }
}
