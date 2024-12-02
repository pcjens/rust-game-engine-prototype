use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::enum_map;
use pal::Pal;

use crate::{Action, ActionKind, Arena, Event, EventQueue, InputDeviceState, QueuedEvent};

/// How much memory is allocated for the frame allocator.
const FRAME_MEM: usize = 1_000_000_000;

#[derive(enum_map::Enum)]
enum TestInput {
    Act,
}

pub struct Engine<'platform> {
    platform: &'platform dyn Pal,
    frame_arena: Arena<'platform>,
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<TestInput>>,
    test_texture: pal::TextureRef,
}

impl Engine<'_> {
    /// Creates a new instance of the engine.
    pub fn new(platform: &dyn Pal) -> Engine {
        let frame_arena = Arena::new(platform, FRAME_MEM)
            .expect("should have enough memory for the frame allocator");

        let test_texture = platform
            .create_texture(
                2,
                2,
                &mut [
                    0xFF, 0xFF, 0x00, 0xFF, // Yellow
                    0xFF, 0x00, 0xFF, 0xFF, // Pink
                    0x00, 0xFF, 0x00, 0xFF, // Green
                    0x00, 0xFF, 0xFF, 0xFF, // Cyan
                ],
            )
            .unwrap();

        Engine {
            platform,
            frame_arena,
            event_queue: ArrayVec::new(),

            test_input: None,
            test_texture,
        }
    }

    /// Runs one iteration of the game loop (called by the platform
    /// implementation).
    pub fn iterate(&mut self, timestamp: Duration) {
        self.frame_arena.reset();

        self.event_queue
            .retain(|queued| !queued.timed_out(timestamp));

        // Testing area follows, could be considered "game code" for now:

        let mut action_test = false;

        if let Some(input) = &mut self.test_input {
            input.update(&mut self.event_queue);
            action_test = input.actions[TestInput::Act].pressed;
        }

        let (w, _) = self.platform.draw_area();
        let w = if action_test { w / 2. } else { w };
        self.platform.draw_triangles(
            &[
                pal::Vertex::new(w / 2. - 200., 200., 0.0, 0.0),
                pal::Vertex::new(w / 2. - 200., 600., 0.0, 1.0),
                pal::Vertex::new(w / 2. + 200., 600., 1.0, 1.0),
                pal::Vertex::new(w / 2. + 200., 200., 1.0, 0.0),
            ],
            &[0, 1, 2, 0, 2, 3],
            pal::DrawSettings {
                texture: Some(self.test_texture),
                ..Default::default()
            },
        );
    }

    /// Handles an event (called by the platform implementation).
    pub fn event(&mut self, event: Event, timestamp: Duration) {
        match event {
            Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                {
                    // TODO: testing code, delete this
                    self.test_input = Some(InputDeviceState {
                        device,
                        actions: enum_map! {
                            TestInput::Act => Action {
                                kind: ActionKind::Held,
                                mapping: self.platform.default_button_for_action(pal::ActionCategory::ActPrimary, device).unwrap(),
                                disabled: false,
                                pressed: false,
                            },
                        },
                    });
                }

                self.event_queue.push(QueuedEvent { event, timestamp });
            }
        }
    }
}
