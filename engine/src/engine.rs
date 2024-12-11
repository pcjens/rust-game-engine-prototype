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
pub struct Engine<'platform, 'internals, 'resources> {
    /// The platform abstraction layer.
    platform: &'platform dyn Pal,
    /// Linear allocator for any persistent data that needs to be dynamically
    /// allocated but does not need to be freed for the entire lifetime of the
    /// engine. Engine internals are suballocated from this.
    persistent_arena: &'internals LinearAllocator<'platform>,
    /// The resource manager for this engine. Used to allocate textures, audio
    /// data, etc. and load them from the disk.
    resources: &'resources Resources<'platform, 'resources>,

    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'platform>,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [crate::input::InputDeviceState], or
    /// after a timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<TestInput>>,
    test_texture: TextureRef,
}

impl Engine<'_, '_, '_> {
    pub const PERSISTENT_MEMORY_SIZE: usize = 1_000_000_000;

    /// Creates a new instance of the engine.
    ///
    /// Pardon the lifetime mess.
    ///
    /// - platform: the platform implementation to be used for this instance of
    ///   the engine.
    /// - persistent_arena: an arena for all the persistent memory the engine
    ///   requires, should be an empty arena with at least
    ///   [Engine::PERSISTENT_MEMORY_SIZE] bytes of capacity. Needs to outlive
    ///   the engine so that engine internals can borrow from it, so it's passed
    ///   in here instead of being created behind the scenes.
    /// - resources: the resource manager used by the engine. Needs to outlive
    ///   the engine so that the game and engine internals can borrow from it,
    ///   so it's passed in here instead of being created behind the scenes.
    pub fn new<'platform, 'internals, 'resources>(
        platform: &'platform dyn Pal,
        persistent_arena: &'internals LinearAllocator<'platform>,
        resources: &'resources Resources<'platform, 'resources>,
    ) -> Engine<'platform, 'internals, 'resources> {
        let frame_arena = LinearAllocator::new(platform, 1_000_000_000)
            .expect("should have enough memory for the frame arena");

        let test_texture = platform.create_texture(2, 2, PixelFormat::Rgba).unwrap();
        let pixels = &[
            0xFF, 0xFF, 0x00, 0xFF, // Yellow
            0xFF, 0x00, 0xFF, 0xFF, // Pink
            0x00, 0xFF, 0x00, 0xFF, // Green
            0x00, 0xFF, 0xFF, 0xFF, // Cyan
        ];
        platform.update_texture(test_texture, 0, 0, 2, 2, pixels);

        Engine {
            platform,
            persistent_arena,
            resources,

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
                Vertex::new(w / 2. - 200., 200., 0.0, 0.0),
                Vertex::new(w / 2. - 200., 600., 0.0, 1.0),
                Vertex::new(w / 2. + 200., 600., 1.0, 1.0),
                Vertex::new(w / 2. + 200., 200., 1.0, 0.0),
            ],
            &[0, 1, 2, 0, 2, 3],
            DrawSettings {
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
                                mapping: self.platform.default_button_for_action(ActionCategory::ActPrimary, device).unwrap(),
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
