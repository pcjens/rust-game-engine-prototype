use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::{EnumArray, EnumMap};
use platform_abstraction_layer::{Button, InputDevice};

const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_millis(200);

pub enum Event {
    DigitalInputPressed(InputDevice, Button),
    DigitalInputReleased(InputDevice, Button),
}

pub type EventQueue = ArrayVec<QueuedEvent, 1000>;

pub struct QueuedEvent {
    pub event: Event,
    pub timestamp: Duration,
}

impl QueuedEvent {
    pub fn timed_out(&self, timestamp: Duration) -> bool {
        if let Some(time_since_event) = timestamp.checked_sub(self.timestamp) {
            time_since_event >= EVENT_QUEUE_TIMEOUT
        } else {
            false
        }
    }
}

pub struct InputDeviceState<K: EnumArray<Action>> {
    pub device: InputDevice,
    pub actions: EnumMap<K, Action>,
}

impl<K: EnumArray<Action>> InputDeviceState<K> {
    pub fn update(&mut self, event_queue: &mut EventQueue) {
        // Reset any instant actions to "not pressed"
        for action in self.actions.values_mut() {
            if matches!(action.kind, ActionKind::Instant) {
                action.pressed = false;
            }
        }

        // Handle events, removing events from the queue if they triggered an action
        event_queue.retain(|event| {
            match event.event {
                Event::DigitalInputPressed(device, button) if device == self.device => {
                    for action in self.actions.values_mut() {
                        if action.mapping == button && !action.disabled {
                            match action.kind {
                                ActionKind::Instant if action.pressed => return true, // handle this event on the next frame, there's many presses queued up
                                ActionKind::Instant => action.pressed = true,
                                ActionKind::Held => action.pressed = true,
                                ActionKind::Toggle => action.pressed = !action.pressed,
                            }
                            return false;
                        }
                    }
                }

                Event::DigitalInputReleased(device, button) if device == self.device => {
                    for action in self.actions.values_mut() {
                        if action.mapping == button && !action.disabled {
                            if matches!(action.kind, ActionKind::Held) {
                                action.pressed = false;
                            }
                            return false;
                        }
                    }
                }

                _ => return true,
            }
            true
        });
    }
}

/// A rebindable action and its current state.
pub struct Action {
    pub kind: ActionKind,
    /// Which button triggers this action
    pub mapping: Button,
    /// If true, events are ignored (but if this gets set to false, events still
    /// in the queue will trigger the action).
    pub disabled: bool,
    pub pressed: bool,
}

pub enum ActionKind {
    /// Actions that happen right away when the button is pressed, and stop
    /// happening until the next press.
    Instant,
    /// Actions that start happening when the button is pressed, and stop
    /// happening when it's released.
    Held,
    /// (Accessible alternative for [`ActionKind::Held`], gameplay logic shouldn't
    /// really change between these two.) Actions that start happening when the
    /// button is pressed one time, and stop happening when it's pressed again.
    Toggle,
}
