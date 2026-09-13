//! Route les évènements d'une manette (lue par `core::gamepad::GamepadPoller`,
//! sans dépendance Slint) vers la cible active. Contrairement au poller, ce
//! module est spécifique à l'application (pile de cibles) et vit donc dans
//! `ui/` plutôt que dans `core/`.

use crate::core::gamepad::{GamepadPoller, GamepadState};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// 8000 sur l'échelle XInput brute (±32767), convertie ici pour l'échelle
/// normalisée de `gilrs` (-1.0..1.0, voir core::gamepad).
const STICK_DEADZONE: f32 = 8000.0 / 32767.0;
/// Délai avant la première répétition, pour qu'un appui bref ne déclenche
/// qu'un seul déplacement. Passé ce délai, la répétition n'a plus de délai
/// propre (voir `decide_moves`) : elle avance à chaque scrutation, comme la
/// répétition clavier. C'est donc `POLL_INTERVAL_MS`, et non une seconde
/// constante, qui fixe la vitesse de répétition maximale.
const NAV_REPEAT_DELAY: Duration = Duration::from_millis(350);
pub const POLL_INTERVAL_MS: u64 = 20;

const DIRECTION_DELTAS: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

/// Cible de navigation manette. Implémentations par défaut vides : une
/// cible ne définit que ce qui a un sens pour elle (un dialogue simple n'a
/// souvent besoin que de `reject`).
pub trait GamepadTarget {
    fn move_selection(&self, _dx: i32, _dy: i32) {}
    fn activate_selection(&self) {}
    fn reject(&self) {}
    fn show_info_for_selection(&self) {}
    fn toggle_fullscreen(&self) {}
}

/// Dpad ou stick combinés : définition unique de "la direction est
/// poussée", partagée par `reseed` et `poll`. Ordre : haut, bas, gauche,
/// droite.
fn read_directions(state: &GamepadState) -> [bool; 4] {
    [
        state.dpad_up || state.stick_y > STICK_DEADZONE,
        state.dpad_down || state.stick_y < -STICK_DEADZONE,
        state.dpad_left || state.stick_x < -STICK_DEADZONE,
        state.dpad_right || state.stick_x > STICK_DEADZONE,
    ]
}

/// Ordre : A, Start (tous deux valident), B (reject), X (info), Back
/// (plein écran).
fn read_buttons(state: &GamepadState) -> [bool; 5] {
    [state.button_a, state.button_start, state.button_b, state.button_x, state.button_back]
}

/// Déplacements (dx, dy) à déclencher d'après l'état précédent et courant
/// des 4 directions, en mettant à jour `last_move` et `repeating`. Séparé
/// de `poll` pour rester testable sans manette réelle. `repeating[i]`
/// distingue l'attente de `NAV_REPEAT_DELAY` du régime de répétition, qui
/// n'a plus de délai propre.
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
            // Premier appui : immédiat, le suivant attendra NAV_REPEAT_DELAY.
            moves.push(DIRECTION_DELTAS[i]);
            last_move[i] = now;
            repeating[i] = false;
        } else if repeating[i] || now.duration_since(last_move[i]) > NAV_REPEAT_DELAY {
            // En répétition : avance à chaque scrutation, sans délai propre.
            moves.push(DIRECTION_DELTAS[i]);
            last_move[i] = now;
            repeating[i] = true;
        }
    }
    moves
}

/// Actions retournées par `decide_button_actions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamepadAction {
    ActivateSelection,
    Reject,
    ShowInfoForSelection,
    ToggleFullscreen,
}

/// Front montant uniquement : les boutons ne répètent jamais. Indices
/// alignés sur `read_buttons`, A et Start partageant la même action.
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

/// Un seul poller et une seule pile de cibles pour toute l'application :
/// les cibles s'empilent via `push_target`/`pop_target`, seule celle du
/// sommet reçoit les évènements, la fenêtre principale restant en bas.
///
/// L'état "maintenu" est resynchronisé sur la manette à chaque changement
/// de cible (voir `reseed`) : le bouton encore enfoncé qui vient d'ouvrir
/// une nouvelle cible est ainsi vu comme maintenu, pas comme un appui neuf
/// que cette cible traiterait aussitôt.
pub struct GamepadRouter {
    poller: GamepadPoller,
    stack: Vec<Rc<dyn GamepadTarget>>,
    held_dirs: [bool; 4],
    held_buttons: [bool; 5],
    last_move: [Instant; 4],
    /// Vrai une fois qu'une direction a répété au moins une fois : les
    /// scrutations suivantes n'attendent plus NAV_REPEAT_DELAY (voir
    /// `decide_moves`). Remis à faux au relâchement.
    repeating: [bool; 4],
}

impl GamepadRouter {
    pub fn new() -> GamepadRouter {
        // Antérieur au délai de répétition, pour que le tout premier appui
        // sur une direction déclenche immédiatement.
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

    /// Cible du sommet si un dialogue est empilé par-dessus la fenêtre
    /// principale, sinon `None`. main.rs suit plutôt `AppState.dialogs`, qui
    /// connaît aussi le type de dialogue ouvert ; cette vue reste
    /// l'invariant que le routeur peut vérifier seul -- jamais consommée
    /// hors des tests, `cfg(test)` plutôt qu'une API publique inutilisée.
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

    /// À appeler à intervalle régulier (voir `POLL_INTERVAL_MS`) depuis un
    /// `slint::Timer`. Met à jour l'état interne et RENVOIE ce qu'il y a à
    /// dispatcher au lieu de le faire elle-même : le routeur est tenu dans
    /// un `Rc<RefCell<..>>`, et dispatcher ici garderait l'emprunt de
    /// `router.borrow_mut().poll()` actif pendant que la cible ouvre ou
    /// ferme un dialogue -- ce qui réemprunte le routeur et panique
    /// (`BorrowMutError`). L'appelant relâche l'emprunt en fin
    /// d'instruction, puis appelle `dispatch`.
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

/// Ce qu'un `GamepadRouter::poll` laisse à dispatcher : des données seules,
/// sans emprunt du routeur (voir `poll` pour le pourquoi).
pub struct PollResult {
    pub target: Rc<dyn GamepadTarget>,
    pub moves: Vec<(i32, i32)>,
    pub actions: Vec<GamepadAction>,
}

/// Dispatche un `PollResult` vers sa cible. Fonction libre et non méthode
/// du routeur, précisément pour pouvoir être appelée après avoir relâché
/// l'emprunt `RefCell` de celui-ci (voir `GamepadRouter::poll`).
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
        // Toujours maintenu, mais le délai de répétition n'est pas écoulé.
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
        // Tenu il y a peu (pas de répétition), mais relâché puis réappuyé.
        let moves = decide_moves([false, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert_eq!(moves, vec![(0, -1)]);
    }

    #[test]
    fn decide_moves_repete_a_chaque_tick_une_fois_engagee() {
        let now = Instant::now();
        let mut last_move = [now - NAV_REPEAT_DELAY * 2; 4];
        let mut repeating = [false; 4];
        // Première répétition, après NAV_REPEAT_DELAY.
        let moves = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now);
        assert_eq!(moves, vec![(0, -1)]);
        assert!(repeating[0]);

        // Un tick plus tard, bien avant NAV_REPEAT_DELAY : doit redéclencher,
        // la répétition engagée n'a plus de délai propre.
        let now2 = now + Duration::from_millis(1);
        let moves2 = decide_moves([true, false, false, false], [true, false, false, false], &mut last_move, &mut repeating, now2);
        assert_eq!(moves2, vec![(0, -1)]);
    }

    #[test]
    fn decide_button_actions_front_montant_uniquement() {
        // Déjà maintenu -- pas de répétition pour les boutons.
        let actions = decide_button_actions([true, false, false, false, false], [true, false, false, false, false]);
        assert!(actions.is_empty());

        // Nouvel appui.
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
        router.poll(); // ne doit pas paniquer même sans manette branchée
    }
}
