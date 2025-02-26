// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

extern crate std;

use core::{cell::Cell, fmt::Arguments, time::Duration};

use platform::{
    ActionCategory, Box, Button, DrawSettings, FileHandle, FileReadTask, InputDevice, InputDevices,
    PixelFormat, Platform, Semaphore, TaskChannel, TextureRef, ThreadState, Vertex, AUDIO_CHANNELS,
    AUDIO_SAMPLE_RATE,
};

/// Simple non-interactive [`Platform`] implementation for use in tests.
#[derive(Debug)]
pub struct TestPlatform {
    current_time: Cell<Duration>,
    threads: usize,
}

impl TestPlatform {
    /// Creates a new [`TestPlatform`], which can be multi-threaded.
    ///
    /// Note that some platforms, like wasm32-unknown-emscripten, don't support
    /// multithreading, which can lead to panics if `multi_threaded` is `true`.
    pub fn new(multi_threaded: bool) -> TestPlatform {
        TestPlatform {
            current_time: Cell::new(Duration::from_millis(0)),
            threads: if multi_threaded { 3 } else { 1 },
        }
    }

    /// Sets the time returned by [`TestPlatform::elapsed`] in milliseconds.
    pub fn set_elapsed_millis(&self, new_millis: u64) {
        self.current_time.set(Duration::from_millis(new_millis));
    }
}

impl Platform for TestPlatform {
    fn draw_area(&self) -> (f32, f32) {
        (320.0, 240.0)
    }

    fn draw_scale_factor(&self) -> f32 {
        1.5
    }

    fn draw_triangles(&self, _vertices: &[Vertex], _indices: &[u32], _settings: DrawSettings) {}

    fn create_texture(&self, width: u16, height: u16, format: PixelFormat) -> Option<TextureRef> {
        let fmt = match format {
            PixelFormat::Rgba => 1,
        };
        Some(TextureRef::new(
            (fmt << 32) | ((width as u64) << 16) | (height as u64),
        ))
    }

    fn update_texture(
        &self,
        texture: TextureRef,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        pixels: &[u8],
    ) {
        let fmt = texture.inner() >> 32;
        let tex_width = ((texture.inner() >> 16) & 0xFFFF) as u16;
        let tex_height = (texture.inner() & 0xFFFF) as u16;
        assert!(x + width <= tex_width, "out of bounds texture update");
        assert!(y + height <= tex_height, "out of bounds texture update");
        match fmt {
            1 => assert_eq!(width as u64 * height as u64 * 4, pixels.len() as u64),
            _ => panic!("got an invalid TextureRef, not from TestPlatform::create_texture"),
        }
    }

    fn open_file(&self, path: &str) -> Option<FileHandle> {
        match path {
            "resources.db" => Some(FileHandle::new(4321)),
            _ => None,
        }
    }

    fn begin_file_read(
        &self,
        file: FileHandle,
        first_byte: u64,
        buffer: Box<[u8]>,
    ) -> FileReadTask {
        FileReadTask::new(file, first_byte, buffer)
    }

    fn is_file_read_finished(&self, _task: &FileReadTask) -> bool {
        true
    }

    fn finish_file_read(&self, task: FileReadTask) -> Result<Box<[u8]>, Box<[u8]>> {
        static RESOURCES_DB: &[u8] = include_bytes!("../../example/resources.db");
        if task.file().inner() != 4321 {
            // Safety: this impl never shares the buffer anywhere.
            return Err(unsafe { task.into_inner() });
        }
        let first_byte = task.task_id() as usize;
        // Safety: never shared this buffer.
        let mut buffer = unsafe { task.into_inner() };
        let len = buffer.len();
        buffer.copy_from_slice(&RESOURCES_DB[first_byte..first_byte + len]);
        Ok(buffer)
    }

    fn create_semaphore(&self) -> Semaphore {
        semaphore::create()
    }

    fn available_parallelism(&self) -> usize {
        self.threads
    }

    fn spawn_pool_thread(&self, channels: [TaskChannel; 2]) -> ThreadState {
        let [(task_sender, mut task_receiver), (mut result_sender, result_receiver)] = channels;
        std::thread::Builder::new()
            .name(std::string::String::from("pool-thread-in-test"))
            .spawn(move || loop {
                let mut task = task_receiver.recv();

                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| task.run()));
                if result.is_err() {
                    // Signal the main thread that we panicked, but don't resume
                    // until we've sent this information back.
                    task.signal_panic();
                }

                'send_result: loop {
                    match result_sender.send(task) {
                        Ok(()) => break 'send_result,
                        Err(task_) => {
                            std::thread::sleep(Duration::from_millis(1));
                            task = task_;
                        }
                    }
                }

                if let Err(err) = result {
                    // We can now resume panicking without causing a hang.
                    std::panic::resume_unwind(err);
                }
            })
            .unwrap();
        ThreadState::new(task_sender, result_receiver)
    }

    fn update_audio_buffer(&self, first_position: u64, samples: &[[i16; AUDIO_CHANNELS]]) {
        let current_position = self.audio_playback_position();
        assert!(
            first_position <= current_position,
            "audio playback underrun",
        );
        assert!(
            first_position + samples.len() as u64 > current_position,
            "only received outdated samples, the audio buffer is too short",
        );
    }

    fn audio_playback_position(&self) -> u64 {
        (self.current_time.get().as_micros() * AUDIO_SAMPLE_RATE as u128 / 1_000_000) as u64
    }

    fn input_devices(&self) -> InputDevices {
        let mut devices = InputDevices::new();
        devices.push(InputDevice::new(1234));
        devices
    }

    fn default_button_for_action(
        &self,
        action: ActionCategory,
        device: InputDevice,
    ) -> Option<Button> {
        match (action, device.inner()) {
            (ActionCategory::ActPrimary, 1234) => Some(Button::new(4321)),
            _ => None,
        }
    }

    fn elapsed(&self) -> Duration {
        self.current_time.get()
    }

    fn println(&self, _message: Arguments) {}

    fn exit(&self, clean: bool) {
        if !clean {
            panic!("TestPlatform::exit({clean}) was called (test ran into an error?)");
        }
    }
}

mod semaphore {
    extern crate std;

    use std::boxed::Box;
    use std::sync::{Condvar, Mutex};

    struct Semaphore {
        value: Mutex<u32>,
        condvar: Condvar,
    }

    impl platform::SemaphoreImpl for Semaphore {
        fn increment(&self) {
            let mut value_lock = self.value.lock().unwrap();
            *value_lock += 1;
            self.condvar.notify_one();
        }

        fn decrement(&self) {
            let mut value_lock = self.value.lock().unwrap();
            while *value_lock == 0 {
                value_lock = self.condvar.wait(value_lock).unwrap();
            }
            *value_lock -= 1;
        }
    }

    pub fn create() -> platform::Semaphore {
        let semaphore: &'static mut Semaphore = Box::leak(Box::new(Semaphore {
            value: Mutex::new(0),
            condvar: Condvar::new(),
        }));
        // Safety: the semaphore is definitely valid for the entire lifetime of
        // the semaphore, since we have a static borrow of it.
        unsafe { platform::Semaphore::new(semaphore, None) }
    }
}
