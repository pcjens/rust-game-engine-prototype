// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use arrayvec::ArrayVec;
use platform::{
    thread_pool::ThreadPool, ActionCategory, EngineCallbacks, Event, Instant, Platform,
};

use crate::{
    allocators::LinearAllocator,
    geom::Rect,
    input::{ActionKind, ActionState, EventQueue, InputDeviceState, QueuedEvent},
    mixer::Mixer,
    multithreading,
    renderer::DrawQueue,
    resources::{
        audio_clip::AudioClipHandle, texture::TextureHandle, FileReader, ResourceDatabase,
        ResourceLoader,
    },
};

#[repr(usize)]
enum TestInput {
    Act,
    _Count,
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
    thread_pool: ThreadPool,
    /// Mixer for playing back audio.
    audio_mixer: Mixer,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [`InputDeviceState`], or after a
    /// timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<{ TestInput::_Count as usize }>>,
    test_texture: TextureHandle,
    test_audio: AudioClipHandle,
}

impl<'eng> Engine<'eng> {
    /// Creates a new instance of the engine.
    ///
    /// - `platform`: the platform implementation to be used for this instance
    ///   of the engine.
    /// - `arena`: an arena for all the persistent memory the engine requires,
    ///   e.g. the resource database. Needs to outlive the engine so that engine
    ///   internals can borrow from it, so it's passed in here instead of being
    ///   created behind the scenes.
    pub fn new(
        platform: &'eng dyn Platform,
        arena: &'static LinearAllocator,
        audio_window_size: usize,
    ) -> Self {
        // TODO: Parameters that should probably be exposed to be tweakable by
        // the game, but are hardcoded here:
        // - Frame arena (or its size)
        // - Asset index (depends on engine memory arena being big enough, the game might want to open the file, and the optimal chunk capacity is game-dependent)
        // - Audio window size
        // - Audio channel count
        // Maybe an EngineConfig struct that has a const function for
        // calculating the memory requirements, so you could
        // "compile-time-static-allocate" the exactly correct amount of memory?

        let thread_pool = multithreading::create_thread_pool(arena, platform, 1)
            .expect("engine arena should have enough memory for the thread pool");

        let frame_arena = LinearAllocator::new(arena, 8 * 1024 * 1024)
            .expect("should have enough memory for the frame arena");

        let db_file = platform
            .open_file("resources.db")
            .expect("resources.db should exist and be readable");
        let mut res_reader = FileReader::new(arena, db_file, 8 * 1024 * 1024, 1024)
            .expect("engine arena should have enough memory for the resource db file reader");
        let resource_db = ResourceDatabase::new(platform, arena, &mut res_reader, 512, 512)
            .expect("engine arena should have enough memory for the resource database");
        let resource_loader = ResourceLoader::new(arena, res_reader, &resource_db)
            .expect("engine arena should have enough memory for the resource loader");

        let audio_mixer = Mixer::new(arena, 1, 64, audio_window_size)
            .expect("engine arena should have enough memory for the audio mixer");

        let test_texture = resource_db.find_texture("testing texture").unwrap();
        let test_audio = resource_db.find_audio_clip("test audio clip").unwrap();

        Engine {
            resource_db,
            resource_loader,
            frame_arena,
            audio_mixer,
            thread_pool,
            event_queue: ArrayVec::new(),

            test_input: None,
            test_texture,
            test_audio,
        }
    }
}

impl EngineCallbacks for Engine<'_> {
    fn iterate(&mut self, platform: &dyn Platform) {
        let timestamp = platform.now();
        self.frame_arena.reset();

        self.resource_loader
            .finish_reads(&mut self.resource_db, platform, 1);

        let scale_factor = platform.draw_scale_factor();
        let mut draw_queue = DrawQueue::new(&self.frame_arena, 100_000, scale_factor).unwrap();

        self.audio_mixer.update_audio_sync(timestamp, platform);

        // Testing area follows, could be considered "game code" for now:

        let mut action_test = false;

        // Handle input
        if let Some(input) = &mut self.test_input {
            input.update(&mut self.event_queue);
            action_test = input.actions[TestInput::Act as usize].pressed;
        }

        if action_test {
            self.audio_mixer
                .play_clip(0, self.test_audio, true, &self.resource_db);
        }

        let test_texture = self.resource_db.get_texture(self.test_texture);
        let mut offset = 0.0;
        for mip in 0..9 {
            let scale = 1. / 2i32.pow(mip) as f32;
            let w = 319.0 * scale;
            let h = 400.0 * scale;
            let draw_success = test_texture.draw(
                Rect::xywh(offset, 0.0, w, h),
                0,
                &mut draw_queue,
                &self.resource_db,
                &mut self.resource_loader,
            );
            assert!(draw_success);
            offset += w + 20.0;
        }

        // /Testing area ends, the following is "end of frame" stuff

        self.event_queue
            .retain(|queued| !queued.timed_out(timestamp));

        draw_queue.dispatch_draw(&self.frame_arena, platform);

        self.audio_mixer.render_audio(
            &mut self.thread_pool,
            platform,
            &self.resource_db,
            &mut self.resource_loader,
        );

        self.resource_loader.dispatch_reads(platform);
    }

    fn event(&mut self, event: Event, timestamp: Instant, platform: &dyn Platform) {
        match event {
            Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                {
                    // TODO: testing code, delete this
                    self.test_input = Some(InputDeviceState {
                        device,
                        actions: [
                            // TestInput::Act
                            ActionState {
                                kind: ActionKind::Instant,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::ActPrimary, device),
                                disabled: false,
                                pressed: false,
                            },
                        ],
                    });
                }

                self.event_queue.push(QueuedEvent { event, timestamp });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use platform::{ActionCategory, EngineCallbacks, Event, Platform};

    use crate::{allocators::LinearAllocator, static_allocator, test_platform::TestPlatform};

    use super::Engine;

    /// Initializes the engine and simulates 4 seconds of running the engine,
    /// with a burst of mashing the "ActPrimary" button in the middle.
    fn run_smoke_test(platform: &TestPlatform, persistent_arena: &'static LinearAllocator) {
        let device = platform.input_devices()[0];
        let button = platform
            .default_button_for_action(ActionCategory::ActPrimary, device)
            .unwrap();

        let mut engine = Engine::new(platform, persistent_arena, 128);

        let fps = 10;
        for current_frame in 0..(4 * fps) {
            platform.set_elapsed_millis(current_frame * 1000 / fps);

            if 2 * fps < current_frame && current_frame < 3 * fps {
                // every three frames, either press down or release the button
                if current_frame % 3 == 0 {
                    engine.event(
                        if current_frame % 2 == 0 {
                            Event::DigitalInputPressed(device, button)
                        } else {
                            Event::DigitalInputReleased(device, button)
                        },
                        platform.now(),
                        platform,
                    );
                }
            }

            engine.iterate(platform);
        }
    }

    #[test]
    #[cfg(not(target_os = "emscripten"))]
    fn smoke_test_multithreaded() {
        static PERSISTENT_ARENA: &LinearAllocator = static_allocator!(64 * 1024 * 1024);
        run_smoke_test(&TestPlatform::new(true), PERSISTENT_ARENA);
    }

    #[test]
    #[ignore = "the emscripten target doesn't support multithreading"]
    #[cfg(target_os = "emscripten")]
    fn smoke_test_multithreaded() {}

    #[test]
    fn smoke_test_singlethreaded() {
        static PERSISTENT_ARENA: &LinearAllocator = static_allocator!(64 * 1024 * 1024);
        run_smoke_test(&TestPlatform::new(false), PERSISTENT_ARENA);
    }
}
