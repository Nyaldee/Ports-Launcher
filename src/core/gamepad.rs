
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
    pub button_y: bool,
    pub button_start: bool,
    pub button_back: bool,
    pub stick_x: f32,
    pub stick_y: f32,
}

pub struct GamepadPoller {
    gilrs: Option<Gilrs>,
}

impl GamepadPoller {
    pub fn new() -> GamepadPoller {
        GamepadPoller { gilrs: Gilrs::new().ok() }
    }

    pub fn is_available(&self) -> bool {
        self.gilrs.is_some()
    }

    pub fn poll(&mut self) -> Option<GamepadState> {
        let gilrs = self.gilrs.as_mut()?;
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
            button_y: gamepad.is_pressed(Button::North),
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
