use pal::{Button, InputDevice};

pub enum Event {
    DigitalInputPressed(InputDevice, Button),
    DigitalInputReleased(InputDevice, Button),
}
