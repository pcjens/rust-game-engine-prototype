use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::enum_map;
use pal::Pal;

use crate::{
    Action, ActionKind, Event, EventQueue, InputDeviceState, LinearAllocator, QueuedEvent,
};

#[derive(enum_map::Enum)]
enum TestInput {
    Act,
}

/// Parts of [Engine] that would make it a self-referential type. First make
/// this, pass then make an engine, and pass this in.
pub struct EngineContext<'platform> {
    /// The platform abstraction layer.
    platform: &'platform dyn Pal,
    /// Linear allocator for dynamically sized but persistent data which does
    /// not need to be freed before engine shutdown.
    persistent_arena: LinearAllocator<'platform>,
}

impl EngineContext<'_> {
    pub fn new(platform: &dyn Pal) -> EngineContext {
        let persistent_arena = LinearAllocator::new(platform, 1_000_000_000)
            .expect("should have enough memory for the persistent arena");
        EngineContext {
            platform,
            persistent_arena,
        }
    }
}

/// The top-level structure of this game engine. Either owns, or has mutably
/// borrowed\* all of the runtime resources and state related to the game
/// engine.
///
/// \*: Since self-referential types are hard, everything that the engine would
/// need to borrow for its entire lifetime are instead stored in
/// [EngineContext], and mutably borrowed by this.
///
/// ## Lifetimes
///
/// `'platform` outlives `'ctx` which outlives the [Engine].
pub struct Engine<'platform, 'ctx> {
    /// The parts of the engine that need to outlive [Engine].
    ctx: &'ctx mut EngineContext<'platform>,

    /// The platform abstraction layer.
    platform: &'platform dyn Pal,
    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'platform>,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [crate::input::InputDeviceState], or
    /// after a timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<TestInput>>,
    test_texture: pal::TextureRef,
}

impl Engine<'_, '_> {
    /// Creates a new instance of the engine.
    pub fn new<'platform, 'ctx>(
        ctx: &'ctx mut EngineContext<'platform>,
    ) -> Engine<'platform, 'ctx> {
        let platform = ctx.platform;
        let frame_arena = LinearAllocator::new(platform, 1_000_000_000)
            .expect("should have enough memory for the frame arena");

        let test_texture = ctx
            .platform
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
            ctx,
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

        let (w, _) = self.ctx.platform.draw_area();
        let w = if action_test { w / 2. } else { w };
        self.ctx.platform.draw_triangles(
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
