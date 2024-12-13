use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::enum_map;
use platform_abstraction_layer::{
    ActionCategory, DrawSettings, Pal, PixelFormat, TextureRef, Vertex,
};

use crate::{
    Action, ActionKind, Event, EventQueue, InputDeviceState, LinearAllocator, QueuedEvent,
    Resources,
};

#[derive(enum_map::Enum)]
enum TestInput {
    Act,
}

/// The top-level structure of the game engine which owns all the runtime state
/// of the game engine and has methods for running the engine.
pub struct Engine<'eng> {
    /// The resource manager for this engine. Used to allocate textures, audio
    /// data, etc. and load them from the disk.
    resources: Resources<'eng>,
    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'eng>,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [`InputDeviceState`], or after a
    /// timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<TestInput>>,
    test_texture: TextureRef,
}

impl<'eng> Engine<'eng> {
    /// Creates a new instance of the engine.
    ///
    /// - `platform`: the platform implementation to be used for this instance
    ///   of the engine.
    /// - `persistent_arena`: an arena for all the persistent memory the engine
    ///   requires, e.g. the resource database. Needs to outlive the engine so
    ///   that engine internals can borrow from it, so it's passed in here
    ///   instead of being created behind the scenes.
    /// - `frame_arena_capacity`: the size of the frame arena.
    pub fn new(
        platform: &'eng dyn Pal,
        persistent_arena: &'eng LinearAllocator,
        frame_arena_size: usize,
    ) -> Self {
        let frame_arena = LinearAllocator::new(platform, frame_arena_size)
            .expect("should have enough memory for the frame arena");

        let resources = Resources::new(platform, persistent_arena)
            .expect("should have enough resource memory to initialize the database");

        // TODO: don't create the texture here directly, use the resource db
        let test_texture = platform.create_texture(2, 2, PixelFormat::Rgba).unwrap();
        let pixels = &[
            0xFF, 0xFF, 0x00, 0xFF, // Yellow
            0xFF, 0x00, 0xFF, 0xFF, // Pink
            0x00, 0xFF, 0x00, 0xFF, // Green
            0x00, 0xFF, 0xFF, 0xFF, // Cyan
        ];
        platform.update_texture(test_texture, 0, 0, 2, 2, pixels);

        Engine {
            resources,
            frame_arena,
            event_queue: ArrayVec::new(),

            test_input: None,
            test_texture,
        }
    }

    /// Runs one iteration of the game loop (called by the platform
    /// implementation).
    pub fn iterate(&mut self, platform: &dyn Pal) {
        let timestamp = platform.elapsed();

        self.frame_arena.reset();

        self.event_queue
            .retain(|queued| !queued.timed_out(timestamp));

        // Testing area follows, could be considered "game code" for now:

        let mut action_test = false;

        if let Some(input) = &mut self.test_input {
            input.update(&mut self.event_queue);
            action_test = input.actions[TestInput::Act].pressed;
        }

        let (w, _) = platform.draw_area();
        let w = if action_test { w / 2. } else { w };
        platform.draw_triangles(
            &[
                Vertex::new(w / 2. - 200., 200., 0.0, 0.0),
                Vertex::new(w / 2. - 200., 600., 0.0, 1.0),
                Vertex::new(w / 2. + 200., 600., 1.0, 1.0),
                Vertex::new(w / 2. + 200., 200., 1.0, 0.0),
            ],
            &[0, 1, 2, 0, 2, 3],
            DrawSettings {
                // TODO: get the texture from the resource db
                texture: Some(self.test_texture),
                ..Default::default()
            },
        );
    }

    /// Handles an event (called by the platform implementation).
    pub fn event(&mut self, event: Event, elapsed: Duration, platform: &dyn Pal) {
        match event {
            Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                {
                    // TODO: testing code, delete this
                    self.test_input = Some(InputDeviceState {
                        device,
                        actions: enum_map! {
                            TestInput::Act => Action {
                                kind: ActionKind::Held,
                                mapping: platform.default_button_for_action(ActionCategory::ActPrimary, device).unwrap(),
                                disabled: false,
                                pressed: false,
                            },
                        },
                    });
                }

                self.event_queue.push(QueuedEvent {
                    event,
                    timestamp: elapsed,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use platform_abstraction_layer::{ActionCategory, Pal};

    use crate::{test_platform::TestPlatform, Event, LinearAllocator};

    use super::Engine;

    /// Initializes the engine and simulates 10 seconds of running the engine,
    /// with a burst of mashing the "ActPrimary" button in the middle.
    #[test]
    pub fn smoke_test() {
        let platform = TestPlatform::new();
        let device = platform.input_devices()[0];
        let button = platform
            .default_button_for_action(ActionCategory::ActPrimary, device)
            .unwrap();

        let persistent_arena = LinearAllocator::new(&platform, 1_000).unwrap();
        let mut engine = Engine::new(&platform, &persistent_arena, 0);

        let fps = 30;
        for current_frame in 0..(10 * fps) {
            platform.set_elapsed_millis(current_frame * 1000 / fps);

            if 4 * fps < current_frame && current_frame < 6 * fps {
                // every three frames, either press down or release the button
                if current_frame % 3 == 0 {
                    engine.event(
                        if current_frame % 2 == 0 {
                            Event::DigitalInputPressed(device, button)
                        } else {
                            Event::DigitalInputReleased(device, button)
                        },
                        platform.elapsed(),
                        &platform,
                    );
                }
            }

            engine.iterate(&platform);
        }
    }
}
