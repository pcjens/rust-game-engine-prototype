/// An input event sent by the platform to the engine for handling.
pub enum Event {
    /// Emitted when a digital input (a button, or a key, but not a thumbstick)
    /// is pressed down.
    DigitalInputPressed(InputDevice, Button),
    /// Emitted when a digital input (a button, or a key, but not a thumbstick)
    /// is pressed released.
    DigitalInputReleased(InputDevice, Button),
}

/// A button or key on a specific input device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Button(u64);

impl Button {
    /// Creates a new [`Button`]. Should only be created in the platform
    /// implementation, which also knows how the inner value is going to be
    /// used.
    pub fn new(id: u64) -> Button {
        Button(id)
    }

    /// Returns the inner value passed into [`Button::new`]. Generally only
    /// relevant to the platform implementation.
    pub fn inner(self) -> u64 {
        self.0
    }
}

/// A specific input device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputDevice(u64);

impl InputDevice {
    /// Creates a new [`InputDevice`]. Should only be created in the platform
    /// implementation, which also knows how the inner value is going to be
    /// used.
    pub fn new(id: u64) -> InputDevice {
        InputDevice(id)
    }

    /// Returns the inner value passed into [`InputDevice::new`]. Generally only
    /// relevant to the platform implementation.
    pub fn inner(self) -> u64 {
        self.0
    }
}

/// Generic action categories for which default buttons are provided. Can be
/// used by games to set up their default mappings for any input device.
/// Different categories may map to the same buttons, so making inputs
/// inputs context-sensitive are recommended.
#[allow(missing_docs)]
pub enum ActionCategory {
    Up,
    Down,
    Right,
    Left,
    Accept,
    Cancel,
    Jump,
    Run,
    ActPrimary,
    ActSecondary,
    Pause,
}
