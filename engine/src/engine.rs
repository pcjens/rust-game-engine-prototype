// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use arrayvec::ArrayVec;
use platform::{
    thread_pool::ThreadPool, ActionCategory, EngineCallbacks, Event, Instant, Platform,
    AUDIO_SAMPLE_RATE,
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

/// Parameters affecting the memory usage of the engine, used in
/// [`Engine::new`].
///
/// Note that while this does cover most persistent memory allocations made by
/// the engine during initialization, it doesn't (currently) cover everything.
/// For example, the memory required by asset metadata is entirely dependent on
/// the amount of assets in the resource database.
#[derive(Clone, Copy)]
pub struct EngineLimits {
    /// The size of the frame arena allocator, in bytes. The frame arena is used
    /// for per-frame memory allocations in rendering, audio playback, and
    /// game-specific uses.
    ///
    /// Defaults to 8 MiB (`8 * 1024 * 1024`).
    pub frame_arena_size: usize,
    /// The maximum amount of concurrently loaded resource chunks. This count,
    /// multiplied by [`CHUNK_SIZE`](crate::resources::CHUNK_SIZE), is the
    /// amount of bytes allocated for non-VRAM based asset memory, like audio
    /// clips being played.
    ///
    /// Defaults to 128.
    pub resource_database_loaded_chunks_count: u32,
    /// The maximum amount of concurrently loaded texture chunks. This,
    /// depending on the platform, will control the amount of VRAM required by
    /// the engine. Each texture chunk's memory requirements depend on the
    /// platform, but each chunk contains texture data with the format and
    /// resolution defined by
    /// [`TEXTURE_CHUNK_FORMAT`](crate::resources::TEXTURE_CHUNK_FORMAT) and
    /// [`TEXTURE_CHUNK_DIMENSIONS`](crate::resources::TEXTURE_CHUNK_DIMENSIONS).
    ///
    /// Defaults to 1024.
    ///
    /// Rationale for the default, just for reference: 1024 texture chunks with
    /// 128x128 resolution, if stored in a tightly packed texture atlas, would
    /// fit exactly in 4096x4096, which is a low enough resolution to be
    /// supported pretty much anywhere with hardware acceleration (Vulkan's
    /// minimum allowed limit is 4096, so any Vulkan-backed platform could
    /// provide this).
    pub resource_database_loaded_texture_chunks_count: u32,
    /// The maximum amount of queued resource database reading operations. This
    /// will generally increase disk read performance by having file reading
    /// operations always queued up, but costs memory and might cause lagspikes
    /// if there's too many chunks to load during a particular frame.
    ///
    /// Defaults to 128.
    pub resource_database_read_queue_capacity: usize,
    /// The size of the buffer used to read data from the resource database, in
    /// bytes. Must be at least [`ResourceDatabase::largest_chunk_source`], but
    /// ideally many times larger, to avoid capping out the buffer before the
    /// read queue is even full.
    ///
    /// Defaults to 8 MiB (`8 * 1024 * 1024`).
    pub resource_database_buffer_size: usize,
    /// The amount of channels the engine's [`Mixer`] has. Each channel can be
    /// individually controlled volume-wise, and all played sounds play on a
    /// specific channel.
    ///
    /// Tip: create an enum for your game's audio channels, and use that enum
    /// when playing back sounds, to have easily refactorable and semantically
    /// meaningful channels. This count should cover all of the enum variants,
    /// e.g. 3 for an enum with 3 variants for 0, 1, and 2.
    ///
    /// Defaults to 1.
    pub audio_channel_count: usize,
    /// The maximum amount of concurrently playing sounds. If more than this
    /// amount of sounds are playing at a time, new sounds might displace old
    /// sounds, or be ignored completely, depending on the parameters of the
    /// sound playback function.
    ///
    /// Defaults to 64.
    pub audio_concurrent_sounds_count: usize,
    /// The amount of samples of audio rendered each frame. Note that this isn't
    /// a traditional "buffer size", where increasing this would increase
    /// latency: the engine can render a lot of audio ahead of time, to avoid
    /// audio cutting off even if the game has lagspikes. In a normal 60 FPS
    /// situation, this length could be 48000, but only the first 800 samples
    /// would be used each frame. The sample rate of the audio is
    /// [`AUDIO_SAMPLE_RATE`].
    ///
    /// Note that this window should be at least long enough to cover audio for
    /// two frames, to avoid audio cutting off due to the platform's audio
    /// callbacks outpacing the once-per-frame audio rendering we do. For a
    /// pessimistic 30 FPS, this would be 3200. The default length is half a
    /// second, i.e. `AUDIO_SAMPLE_RATE / 2`.
    pub audio_window_length: usize,
}

impl EngineLimits {
    /// The default configuration for the engine used in its unit tests.
    pub const DEFAULT: EngineLimits = EngineLimits {
        frame_arena_size: 8 * 1024 * 1024,
        resource_database_loaded_chunks_count: 128,
        resource_database_loaded_texture_chunks_count: 512,
        resource_database_read_queue_capacity: 128,
        resource_database_buffer_size: 8 * 1024 * 1024,
        audio_channel_count: 1,
        audio_concurrent_sounds_count: 64,
        audio_window_length: (AUDIO_SAMPLE_RATE / 2) as usize,
    };
}

impl Default for EngineLimits {
    fn default() -> Self {
        EngineLimits::DEFAULT
    }
}

/// The top-level structure of the game engine which owns all the runtime state
/// of the game engine and has methods for running the engine.
pub struct Engine<'a> {
    /// Database of the non-code parts of the game, e.g. textures.
    resource_db: ResourceDatabase,
    /// Queue of loading tasks which insert loaded chunks into the `resource_db`
    /// occasionally.
    resource_loader: ResourceLoader,
    /// Linear allocator for any frame-internal dynamic allocation needs.
    frame_arena: LinearAllocator<'a>,
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
    test_counter: u32,
}

impl Engine<'_> {
    /// Creates a new instance of the engine.
    ///
    /// - `platform`: the platform implementation to be used for this instance
    ///   of the engine.
    /// - `arena`: an arena for all the persistent memory the engine requires,
    ///   e.g. the resource database.
    /// - `limits`: defines the limits for the various subsystems of the engine,
    ///   for dialing in the appropriate tradeoffs between memory usage and game
    ///   requirements.
    pub fn new(
        platform: &dyn Platform,
        arena: &'static LinearAllocator,
        limits: EngineLimits,
    ) -> Self {
        let thread_pool = multithreading::create_thread_pool(arena, platform, 1)
            .expect("engine arena should have enough memory for the thread pool");

        let frame_arena = LinearAllocator::new(arena, limits.frame_arena_size)
            .expect("should have enough memory for the frame arena");

        let db_file = platform
            .open_file("resources.db")
            .expect("resources.db should exist and be readable");

        let mut res_reader = FileReader::new(
            arena,
            db_file,
            limits.resource_database_buffer_size,
            limits.resource_database_read_queue_capacity,
        )
        .expect("engine arena should have enough memory for the resource db file reader");

        let resource_db = ResourceDatabase::new(
            platform,
            arena,
            &mut res_reader,
            limits.resource_database_loaded_chunks_count,
            limits.resource_database_loaded_texture_chunks_count,
        )
        .expect("engine arena should have enough memory for the resource database");

        let resource_loader = ResourceLoader::new(arena, res_reader, &resource_db)
            .expect("engine arena should have enough memory for the resource loader");

        let audio_mixer = Mixer::new(
            arena,
            limits.audio_channel_count,
            limits.audio_concurrent_sounds_count,
            limits.audio_window_length,
        )
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
            test_counter: 0,
        }
    }
}

impl EngineCallbacks for Engine<'_> {
    fn iterate(&mut self, platform: &dyn Platform) {
        let timestamp = platform.now();
        self.frame_arena.reset();

        self.resource_loader
            .finish_reads(&mut self.resource_db, platform, 128);

        self.resource_db.chunks.increment_ages();
        self.resource_db.texture_chunks.increment_ages();

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
            self.test_counter += 1;
        }

        let test_texture = self.resource_db.get_texture(self.test_texture);
        let mut offset = 0.0;
        for mip in 0..9 {
            if self.test_counter % 9 > mip {
                continue;
            }
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

    use super::{Engine, EngineLimits};

    /// Initializes the engine and simulates 4 seconds of running the engine,
    /// with a burst of mashing the "ActPrimary" button in the middle.
    fn run_smoke_test(platform: &TestPlatform, persistent_arena: &'static LinearAllocator) {
        let device = platform.input_devices()[0];
        let button = platform
            .default_button_for_action(ActionCategory::ActPrimary, device)
            .unwrap();

        let mut engine = Engine::new(
            platform,
            persistent_arena,
            EngineLimits {
                audio_window_length: 128,
                ..EngineLimits::DEFAULT
            },
        );

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
