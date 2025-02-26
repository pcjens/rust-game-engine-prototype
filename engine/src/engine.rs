// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::time::Duration;

use arrayvec::ArrayVec;
use platform::{
    thread_pool::ThreadPool, ActionCategory, EngineCallbacks, Event, Platform, AUDIO_CHANNELS,
    AUDIO_SAMPLE_RATE,
};

use crate::{
    allocators::LinearAllocator,
    collections::FixedVec,
    geom::Rect,
    input::{ActionKind, ActionState, EventQueue, InputDeviceState, QueuedEvent},
    multithreading::{self, parallelize},
    renderer::DrawQueue,
    resources::{texture::TextureHandle, ResourceDatabase, ResourceLoader},
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
    /// Buffer for storing audio samples sent to the platform each frame.
    ///
    /// Note that this isn't "buffer" in the sense of pro audio "buffer size"
    /// parameters where the latency rises as the size grows: the actual latency
    /// depends on the platform implementation and its audio buffer size. This
    /// buffer is rewritten every frame to match "whatever noises would play
    /// over the next N milliseconds, starting from this frame," and then sent
    /// to the platform so it can consume from it until it gets another update
    /// next frame.
    ///
    /// TODO: move this to a proper audio subsystem
    audio_buffer: FixedVec<'static, [i16; AUDIO_CHANNELS]>,
    /// Queued up events from the platform layer. Discarded after being used by
    /// the game to trigger an action via [`InputDeviceState`], or after a
    /// timeout if not.
    event_queue: EventQueue,

    test_input: Option<InputDeviceState<{ TestInput::_Count as usize }>>,
    test_texture: TextureHandle,
    test_input_time: Duration,
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
    pub fn new(
        platform: &'eng dyn Platform,
        persistent_arena: &'static LinearAllocator,
        audio_window_size: usize,
    ) -> Self {
        // TODO: Parameters that should probably be exposed to be tweakable by
        // the game, but are hardcoded here:
        // - Frame arena (or its size)
        // - Asset index (depends on persistent arena being big enough, the game might want to open the file, and the optimal chunk capacity is game-dependent)
        // - Audio window size
        // Maybe an EngineConfig struct that has a const function for
        // calculating the memory requirements, so you could
        // "compile-time-static-allocate" the exactly correct amount of memory?

        let thread_pool = multithreading::create_thread_pool(persistent_arena, platform, 1)
            .expect("persistent arena should have enough memory for the thread pool");

        let frame_arena = LinearAllocator::new(persistent_arena, 8 * 1024 * 1024)
            .expect("should have enough memory for the frame arena");

        let db_file = platform
            .open_file("resources.db")
            .expect("resources.db should exist and be readable");
        let resource_db = ResourceDatabase::new(platform, persistent_arena, db_file, 512, 512)
            .expect("persistent arena should have enough memory for the resource database");
        let staging_size = 8 * 1024 * 1024;
        let resource_loader = ResourceLoader::new(persistent_arena, staging_size, &resource_db)
            .expect("persistent arena should have enough memory for the resource loader");

        let mut audio_buffer = FixedVec::new(persistent_arena, audio_window_size)
            .expect("persistent arena should have enough memory for the audio buffer");
        audio_buffer.fill_with_zeroes();

        let test_texture = resource_db.find_texture("testing texture").unwrap();

        Engine {
            resource_db,
            resource_loader,
            frame_arena,
            audio_buffer,
            thread_pool,
            event_queue: ArrayVec::new(),

            test_input: None,
            test_texture,
            test_input_time: platform.elapsed(),
        }
    }
}

impl EngineCallbacks for Engine<'_> {
    fn iterate(&mut self, platform: &dyn Platform) {
        let timestamp = platform.elapsed();
        self.frame_arena.reset();

        self.resource_loader
            .finish_reads(&mut self.resource_db, platform);

        let scale_factor = platform.draw_scale_factor();
        let mut draw_queue = DrawQueue::new(&self.frame_arena, 100_000, scale_factor).unwrap();

        // Testing area follows, could be considered "game code" for now:

        let mut action_test = false;

        // Handle input
        if let Some(input) = &mut self.test_input {
            input.update(&mut self.event_queue);
            action_test = input.actions[TestInput::Act as usize].pressed;
        }

        if action_test && platform.elapsed() - self.test_input_time > Duration::from_millis(100) {
            self.test_input_time = platform.elapsed();
        }

        let test_texture = self.resource_db.get_texture(self.test_texture);
        let (screen_width, _) = platform.draw_area();
        let x = if action_test { -screen_width } else { 0.0 };
        let mut offset = 0.0;
        for mip in 0..9 {
            let scale = 1. / 2i32.pow(mip) as f32;
            let w = 319.0 * scale;
            let h = 400.0 * scale;
            let draw_success = test_texture.draw(
                Rect::xywh(x + offset, 0.0, w, h),
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

        self.resource_loader
            .dispatch_reads(&self.resource_db, platform);

        // TODO: add a system for synchronizing game time with audio playback
        // time (and decide how to handle lagspikes, which shouldn't progress
        // game time a lot, but they definitely do progress audio playback time)

        // TODO: add a proper system that maintains a list of playing sounds

        let audio_pos = platform.audio_playback_position();
        parallelize(
            &mut self.thread_pool,
            &mut self.audio_buffer,
            move |buf, offset| {
                for (t, sample) in buf.iter_mut().enumerate() {
                    // TODO: replace with a more natural sounding noise to detect issues easier
                    fn triangle(x: u32) -> i64 {
                        let x = (x % AUDIO_SAMPLE_RATE) as i64;
                        let amplitude = i16::MAX as i64;
                        (amplitude - x * 2 * amplitude).abs() / AUDIO_SAMPLE_RATE as i64
                    }
                    let t = audio_pos as u32 + (offset + t) as u32;
                    let s = triangle(t * 220) as i16 / 20;
                    *sample = [s, s];
                }
            },
        )
        .unwrap();
        platform.update_audio_buffer(audio_pos, &self.audio_buffer);
    }

    fn event(&mut self, event: Event, elapsed: Duration, platform: &dyn Platform) {
        match event {
            Event::DigitalInputPressed(device, _) | Event::DigitalInputReleased(device, _) => {
                {
                    // TODO: testing code, delete this
                    self.test_input = Some(InputDeviceState {
                        device,
                        actions: [
                            // TestInput::Act
                            ActionState {
                                kind: ActionKind::Held,
                                mapping: platform
                                    .default_button_for_action(ActionCategory::ActPrimary, device),
                                disabled: false,
                                pressed: false,
                            },
                        ],
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
                        platform.elapsed(),
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
