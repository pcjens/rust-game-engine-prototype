use core::time::Duration;

use arrayvec::ArrayVec;
use enum_map::enum_map;
use platform_abstraction_layer::{ActionCategory, EngineCallbacks, Event, Pal};

use crate::{
    renderer::DrawQueue,
    resources::{
        assets::TextureHandle,
        chunks::{LoadedTextureChunk, TextureChunkDescriptor},
        ResourceDatabase, TEXTURE_CHUNK_DIMENSIONS, TEXTURE_CHUNK_FORMAT,
    },
    Action, ActionKind, EventQueue, FixedVec, InputDeviceState, LinearAllocator, QueuedEvent,
};

#[derive(enum_map::Enum)]
enum TestInput {
    Act,
}

/// The top-level structure of the game engine which owns all the runtime state
/// of the game engine and has methods for running the engine.
pub struct Engine<'eng> {
    resource_db: ResourceDatabase<'eng>,
    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'eng>,
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
    pub fn new(platform: &'eng dyn Pal, persistent_arena: &'eng LinearAllocator) -> Self {
        // TODO: Parameters that should probably be exposed to be tweakable by
        // the game, but are hardcoded here:
        // - Frame arena (or its size)
        // - Asset index (depends on persistent arena being big enough, the game might want to open the file, and the optimal chunk capacity is game-dependent)

        let mut frame_arena = LinearAllocator::new(platform, 1024 * 1024 * 1024)
            .expect("should have enough memory for the frame arena");

        let db_file = platform
            .open_file("resources.db")
            .expect("resources.db should exist and be readable");
        let resource_db =
            ResourceDatabase::new(platform, persistent_arena, &frame_arena, db_file, 16, 128)
                .expect("persistent arena should have enough memory for asset db");

        frame_arena.reset();

        let test_texture = resource_db.find_texture("testing texture").unwrap();

        Engine {
            resource_db,
            frame_arena,
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
        self.event_queue
            .retain(|queued| !queued.timed_out(timestamp));

        let mut draw_queue = DrawQueue::new(&self.frame_arena).unwrap();

        // TODO: Some kind of "chunk load queue" instead of this
        let mut texture_chunk_load_requests = FixedVec::new(
            &self.frame_arena,
            self.resource_db.texture_chunk_descriptors.len(),
        )
        .unwrap();

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
            [w / 2., 200.0, 400.0, 400.0],
            0,
            &mut draw_queue,
            &self.resource_db,
            &mut texture_chunk_load_requests,
        );

        // TODO: move  this somewhere else
        for requested_chunk_idx in texture_chunk_load_requests.iter() {
            let TextureChunkDescriptor {
                region_width,
                region_height,
                source_bytes,
            } = &self.resource_db.texture_chunk_descriptors[*requested_chunk_idx as usize];

            // Load the pixels from disk
            let first_byte = self.resource_db.chunk_data_offset + source_bytes.start;
            let len = (source_bytes.end - source_bytes.start) as usize;
            let mut buffer = FixedVec::new(&self.frame_arena, len).unwrap();
            buffer.fill_with_zeroes();
            let mut read_task =
                platform.begin_file_read(self.resource_db.chunk_data_file, first_byte, &mut buffer);
            let pixels = loop {
                match platform.poll_file_read(read_task) {
                    Ok(pixels) => break pixels,
                    Err(Some(task)) => read_task = task,
                    Err(None) => panic!(),
                }
            };

            // Allocate the texture chunk
            let create_tex = || {
                let (w, h) = TEXTURE_CHUNK_DIMENSIONS;
                let texture = platform.create_texture(w, h, TEXTURE_CHUNK_FORMAT)?;
                Some(LoadedTextureChunk(texture))
            };
            if let Some(texture) = self
                .resource_db
                .texture_chunks
                .insert(*requested_chunk_idx, create_tex)
            {
                // Write the data to the texture chunk
                platform.update_texture(texture.0, 0, 0, *region_width, *region_height, pixels);
            }
        }

        draw_queue.dispatch_draw(&self.frame_arena, platform);
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

    use crate::{test_platform::TestPlatform, LinearAllocator};

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

        let persistent_arena = LinearAllocator::new(&platform, 50_000_000).unwrap();
        let mut engine = Engine::new(&platform, &persistent_arena);

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
