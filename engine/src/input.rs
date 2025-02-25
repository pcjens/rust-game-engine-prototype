// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::time::Duration;

use arrayvec::ArrayVec;
use platform_abstraction_layer::{Button, Event, InputDevice};

/// The amount of time [`QueuedEvent`]s are held in the [`EventQueue`] without
/// being handled.
pub const EVENT_QUEUE_TIMEOUT: Duration = Duration::from_millis(200);

/// A queue of input events to be processed by [`InputDeviceState::update`].
pub type EventQueue = ArrayVec<QueuedEvent, 1000>;

/// Input event that happened at some point in the past, waiting to be used as a
/// trigger for an [`ActionState`], or to be timed out.
pub struct QueuedEvent {
    /// The event itself.
    pub event: Event,
    /// Timestamp of when the event happened.
    // TODO: don't use Duration as a timestamp type in general
    pub timestamp: Duration,
}

impl QueuedEvent {
    /// Returns true if the time between this event and the given timestamp is
    /// greater than [`EVENT_QUEUE_TIMEOUT`].
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
/// `N` should be the amount of actions that can be triggered by the player to
/// be detected by the game. Every frame, [`InputDeviceState::update`] should be
/// called, and then the [`ActionState::pressed`] status of the values in
/// [`InputDeviceState::actions`] should be used to trigger any relevant events
/// in the game based on the actions' index.
///
/// ### Example
/// ```
/// # let mut event_queue = engine::input::EventQueue::new();
/// # let a_device_from_platform = platform_abstraction_layer::InputDevice::new(0);
/// # let button_from_platform = platform_abstraction_layer::Button::new(0);
/// # let another_button_from_platform = platform_abstraction_layer::Button::new(0);
/// use engine::input::{InputDeviceState, ActionState, ActionKind};
///
/// #[repr(usize)]
/// enum PlayerAction {
///     Jump,
///     Run,
///     Select,
///     _Count,
/// }
///
/// let mut input_device_state = InputDeviceState {
///     device: a_device_from_platform,
///     actions: [ActionState::default(); PlayerAction::_Count as usize],
/// };
///
/// // Maybe bind the actions:
/// let jump_action = &mut input_device_state.actions[PlayerAction::Jump as usize];
/// jump_action.kind = ActionKind::Instant;
/// jump_action.mapping = Some(button_from_platform);
///
/// let run_action = &mut input_device_state.actions[PlayerAction::Run as usize];
/// run_action.kind = ActionKind::Held;
/// run_action.mapping = Some(another_button_from_platform);
///
/// // Somewhere early in a frame:
/// input_device_state.update(&mut event_queue);
/// if input_device_state.actions[PlayerAction::Jump as usize].pressed {
///     // Jump!
/// }
/// ```
pub struct InputDeviceState<const N: usize> {
    /// The device this [`InputDeviceState`] tracks.
    pub device: InputDevice,
    /// Each action's current state, updated based on events in
    /// [`InputDeviceState::update`].
    pub actions: [ActionState; N],
}

impl<const N: usize> InputDeviceState<N> {
    /// Checks the event queue for any events that could be consumed by this
    /// [`InputDeviceState`], and consumes any such events to trigger actions.
    ///
    /// Also resets the [`ActionState::pressed`] status of
    /// [`ActionKind::Instant`] actions.
    pub fn update(&mut self, event_queue: &mut EventQueue) {
        // Reset any instant actions to "not pressed"
        for action in &mut self.actions {
            if matches!(action.kind, ActionKind::Instant) {
                action.pressed = false;
            }
        }

        // Handle events, removing events from the queue if they triggered an action
        event_queue.retain(|event| {
            match event.event {
                Event::DigitalInputPressed(device, button) if device == self.device => {
                    for action in &mut self.actions {
                        if action.mapping == Some(button) && !action.disabled {
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
                    for action in &mut self.actions {
                        if action.mapping == Some(button) && !action.disabled {
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
#[derive(Clone, Copy, Default)]
pub struct ActionState {
    /// How events are used to change the status of [`ActionState::pressed`].
    pub kind: ActionKind,
    /// Button which triggers this action.
    pub mapping: Option<Button>,
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
#[derive(Clone, Copy, Default)]
pub enum ActionKind {
    /// Actions that happen right away when the button is pressed, and stop
    /// happening until the next press.
    #[default]
    Instant,
    /// Actions that start happening when the button is pressed, and stop
    /// happening when it's released.
    Held,
    /// (Accessible alternative for [`ActionKind::Held`], gameplay logic shouldn't
    /// really change between these two.) Actions that start happening when the
    /// button is pressed one time, and stop happening when it's pressed again.
    Toggle,
}
