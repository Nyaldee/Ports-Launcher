//! Lecture de l'état manette (dpad, stick gauche, boutons A/B/X/Start/Back)
//! via `gilrs` -- XInput sur Windows, cross-platform ailleurs : préféré au
//! crate `windows` en direct pour garder la porte ouverte à un futur portage
//! Linux sans code supplémentaire ici.
//!
//! Une seule manette lue (la première connectée). La logique de deadzone/
//! répétition/pile de cibles vit dans `ui::gamepad_router` -- ce module
//! n'expose qu'un instantané brut de l'état.

use gilrs::{Axis, Button, Gilrs};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GamepadState {
    pub dpad_up: bool,
    pub dpad_down: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub button_a: bool,
    pub button_b: bool,
    pub button_x: bool,
    pub button_start: bool,
    pub button_back: bool,
    /// Normalisé entre -1.0 et 1.0 (gilrs), pas la plage brute ±32767 de
    /// XInput -- les seuils de deadzone côté `ui::gamepad_router` doivent
    /// être exprimés dans cette même échelle normalisée.
    pub stick_x: f32,
    pub stick_y: f32,
}

pub struct GamepadPoller {
    gilrs: Option<Gilrs>,
}

impl GamepadPoller {
    /// `gilrs` peut échouer à s'initialiser (API plateforme absente/trop
    /// ancienne) -- traité comme "pas de manette disponible", jamais
    /// fatal.
    pub fn new() -> GamepadPoller {
        GamepadPoller { gilrs: Gilrs::new().ok() }
    }

    pub fn is_available(&self) -> bool {
        self.gilrs.is_some()
    }

    /// `None` si aucune manette n'est branchée (ou si l'initialisation a
    /// échoué) -- l'appelant garde alors son dernier état connu plutôt que
    /// de le remettre à zéro à chaque scrutation (une manette débranchée
    /// puis rebranchée ne doit pas réinitialiser une navigation en cours).
    pub fn poll(&mut self) -> Option<GamepadState> {
        let gilrs = self.gilrs.as_mut()?;
        // Vide la file d'évènements internes -- gilrs ne met à jour l'état
        // interrogé ensuite (is_pressed/value) qu'au fil de next_event().
        while gilrs.next_event().is_some() {}
        let (_, gamepad) = gilrs.gamepads().next()?;
        Some(GamepadState {
            dpad_up: gamepad.is_pressed(Button::DPadUp),
            dpad_down: gamepad.is_pressed(Button::DPadDown),
            dpad_left: gamepad.is_pressed(Button::DPadLeft),
            dpad_right: gamepad.is_pressed(Button::DPadRight),
            button_a: gamepad.is_pressed(Button::South),
            button_b: gamepad.is_pressed(Button::East),
            button_x: gamepad.is_pressed(Button::West),
            button_start: gamepad.is_pressed(Button::Start),
            button_back: gamepad.is_pressed(Button::Select),
            stick_x: gamepad.value(Axis::LeftStickX),
            stick_y: gamepad.value(Axis::LeftStickY),
        })
    }
}

impl Default for GamepadPoller {
    fn default() -> Self {
        Self::new()
    }
}
