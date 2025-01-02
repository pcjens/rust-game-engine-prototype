use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::{EnumArray, EnumMap};
use platform_abstraction_layer::{Button, Event, InputDevice};

const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_millis(200);

/// A queue of input events to be processed by [`InputDeviceState::update`].
pub type EventQueue = ArrayVec<QueuedEvent, 1000>;

/// Input event that happened at some point in the past, waiting to be used as a
/// trigger for an [`Action`], or to be timed out.
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

/// The main input interface for the game, created, and maintained in game code.
///
/// `K` should be an enum that represents the various actions that can be
/// triggered by the player to be detected by the game. Every frame,
/// [`InputDeviceState::update`] should be called to update action states, and
/// then the [`Action::pressed`] status of the values in
/// [`InputDeviceState::actions`] should be used to trigger any relevant events
/// in the game.
pub struct InputDeviceState<K: EnumArray<Action>> {
    /// The device this [`InputDeviceState`] tracks.
    pub device: InputDevice,
    /// Each action's current state, updated based on events in
    /// [`InputDeviceState::update`].
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
    /// How events are used to change the status of [`Action::pressed`].
    pub kind: ActionKind,
    /// Button which triggers this action.
    pub mapping: Button,
    /// If true, events are ignored, but unless the events time out, they will
    /// trigger the action once this is set to false again.
    ///
    /// Can be used to e.g. disable jumping while in-air, but still cause a jump
    /// trigger if the player pressed the button right before landing.
    pub disabled: bool,
    /// True if the action should be triggered based on input events, parsed
    /// according to the action's [`ActionKind`].
    pub pressed: bool,
}

/// The button press pattern to be used to trigger a specific action.
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
