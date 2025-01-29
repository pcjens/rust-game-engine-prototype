use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::enum_map;
use platform_abstraction_layer::{
    thread_pool::ThreadPool, ActionCategory, EngineCallbacks, Event, Pal,
};

use crate::{
    allocators::{LinearAllocator, StaticAllocator},
    input::{Action, ActionKind, EventQueue, InputDeviceState, QueuedEvent},
    multithreading::{self, thread_pool_scope},
    renderer::DrawQueue,
    resources::{assets::TextureHandle, ResourceDatabase, ResourceLoader},
};

#[derive(enum_map::Enum)]
enum TestInput {
    Act,
}

/// The top-level structure of the game engine which owns all the runtime state
/// of the game engine and has methods for running the engine.
pub struct Engine<'eng> {
    /// Database of the non-code parts of the game, e.g. textures.
    resource_db: ResourceDatabase,
    /// Queue of loading tasks which insert loaded chunks into the `resource_db`
    /// occasionally.
    resource_loader: ResourceLoader,
    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'eng>,
    /// Thread pool for splitting compute-heavy workloads to multiple threads.
    #[allow(unused)]
    thread_pool: ThreadPool,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [`InputDeviceState`], or after a
    /// timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<TestInput>>,
    test_texture: TextureHandle,
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
    pub fn new(platform: &'eng dyn Pal, persistent_arena: &'static StaticAllocator) -> Self {
        // TODO: Parameters that should probably be exposed to be tweakable by
        // the game, but are hardcoded here:
        // - Frame arena (or its size)
        // - Asset index (depends on persistent arena being big enough, the game might want to open the file, and the optimal chunk capacity is game-dependent)

        let thread_pool = multithreading::create_thread_pool(persistent_arena, platform, 4)
            .expect("persistent arena should have enough memory for the thread pool");

        let frame_arena = LinearAllocator::new(platform, 10_000)
            .expect("should have enough memory for the frame arena");

        let db_file = platform
            .open_file("resources.db")
            .expect("resources.db should exist and be readable");
        let resource_db = ResourceDatabase::new(platform, persistent_arena, db_file, 1, 1)
            .expect("persistent arena should have enough memory for the resource database");
        let resource_loader = ResourceLoader::new(persistent_arena, 20, &resource_db)
            .expect("persistent arena should have enough memory for the resource loader");

        let test_texture = resource_db.find_texture("testing texture").unwrap();

        Engine {
            resource_db,
            resource_loader,
            frame_arena,
            thread_pool,
            event_queue: ArrayVec::new(),

            test_input: None,
            test_texture,
        }
    }
}

impl EngineCallbacks for Engine<'_> {
    fn iterate(&mut self, platform: &dyn Pal) {
        let timestamp = platform.elapsed();
        self.frame_arena.reset();

        let mut test_numbers = [1i32; 100];
        thread_pool_scope(&mut self.thread_pool, &self.frame_arena, |thread_pool| {
            let test_numbers = thread_pool
                .scatter(&mut test_numbers, |numbers| {
                    for number in numbers {
                        *number *= 2;
                    }
                })
                .unwrap();

            self.event_queue
                .retain(|queued| !queued.timed_out(timestamp));

            self.resource_loader
                .finish_reads(&mut self.resource_db, platform);

            let mut draw_queue = DrawQueue::new(&self.frame_arena, 1).unwrap();

            // Testing area follows, could be considered "game code" for now:

            let mut action_test = false;

            if let Some(input) = &mut self.test_input {
                input.update(&mut self.event_queue);
                action_test = input.actions[TestInput::Act].pressed;
            }

            let test_texture = self.resource_db.get_texture(self.test_texture);
            let (w, _) = platform.draw_area();
            let w = if action_test { w / 2. } else { w };
            test_texture.draw(
                (w / 2., 200.0, 400.0, 400.0),
                0,
                &mut draw_queue,
                &self.resource_db,
                &mut self.resource_loader,
            );

            draw_queue.dispatch_draw(&self.frame_arena, platform);

            self.resource_loader
                .dispatch_reads(&self.resource_db, platform);

            let test_numbers = thread_pool.gather(test_numbers).unwrap();
            assert!(test_numbers.iter().all(|i| *i == 2));
        })
        .unwrap();
    }

    fn event(&mut self, event: Event, elapsed: Duration, platform: &dyn Pal) {
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
    use platform_abstraction_layer::{ActionCategory, EngineCallbacks, Event, Pal};

    use crate::{allocators::StaticAllocator, static_allocator, test_platform::TestPlatform};

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

        static PERSISTENT_ARENA: &StaticAllocator = static_allocator!(100_000);
        let mut engine = Engine::new(&platform, PERSISTENT_ARENA);

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
