use platform_abstraction_layer::{Button, InputDevice};

pub enum Event {
    DigitalInputPressed(InputDevice, Button),
    DigitalInputReleased(InputDevice, Button),
}
