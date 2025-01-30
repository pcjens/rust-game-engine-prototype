#[cfg(feature = "std")]
extern crate std;

use core::{cell::Cell, ffi::c_void, fmt::Arguments, time::Duration};

use platform_abstraction_layer::{
    ActionCategory, Box, Button, DrawSettings, FileHandle, FileReadTask, InputDevice, InputDevices,
    Pal, PixelFormat, Semaphore, TaskChannel, TextureRef, ThreadState, Vertex,
};

#[derive(Debug)]
pub struct TestPlatform {
    current_time: Cell<Duration>,
}

impl TestPlatform {
    pub fn new() -> TestPlatform {
        TestPlatform {
            current_time: Cell::new(Duration::from_millis(0)),
        }
    }

    pub fn set_elapsed_millis(&self, new_millis: u64) {
        self.current_time.set(Duration::from_millis(new_millis));
    }
}

impl Pal for TestPlatform {
    fn draw_area(&self) -> (f32, f32) {
        (320.0, 240.0)
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
        static RESOURCES_DB: &[u8] = include_bytes!("../../resources.db");
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

    #[cfg(not(feature = "std"))]
    fn create_semaphore(&self) -> Semaphore {
        Semaphore::single_threaded()
    }

    #[cfg(not(feature = "std"))]
    fn thread_pool_size(&self) -> Option<usize> {
        None
    }

    #[cfg(not(feature = "std"))]
    fn spawn_pool_thread(&self, _channels: [TaskChannel; 2]) -> ThreadState {
        unimplemented!("TestPlatform is single-threaded")
    }

    #[cfg(feature = "std")]
    fn create_semaphore(&self) -> Semaphore {
        semaphore::create()
    }

    #[cfg(feature = "std")]
    fn thread_pool_size(&self) -> Option<usize> {
        // Ideally this would be a constant, but in case the actual available
        // parallellism is just 1, the thread pool could deadlock.
        let parallellism = std::thread::available_parallelism().ok()?.get();
        if parallellism > 1 {
            Some(3)
        } else {
            None
        }
    }

    #[cfg(feature = "std")]
    fn spawn_pool_thread(&self, channels: [TaskChannel; 2]) -> ThreadState {
        let [(task_sender, mut task_receiver), (mut result_sender, result_receiver)] = channels;
        std::thread::Builder::new()
            .name(std::string::String::from("pool-thread-in-test"))
            .spawn(move || loop {
                let mut task = task_receiver.recv();
                task.run();
                'send_result: loop {
                    match result_sender.send(task) {
                        Ok(()) => break 'send_result,
                        Err(task_) => {
                            std::thread::sleep(Duration::from_millis(1));
                            task = task_;
                        }
                    }
                }
            })
            .unwrap();
        ThreadState::new(task_sender, result_receiver)
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

    fn malloc(&self, size: usize) -> *mut c_void {
        // Safety: ffi call, handling the possible null pointer is up to the caller
        unsafe { libc::malloc(size) }
    }

    unsafe fn free(&self, ptr: *mut c_void) {
        libc::free(ptr);
    }
}

#[cfg(feature = "std")]
mod semaphore {
    extern crate std;

    use std::boxed::Box;
    use std::sync::{Condvar, Mutex};

    use platform_abstraction_layer as pal;

    struct Semaphore {
        value: Mutex<u32>,
        condvar: Condvar,
    }

    impl pal::SemaphoreImpl for Semaphore {
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

    pub fn create() -> pal::Semaphore {
        let semaphore: &'static mut Semaphore = Box::leak(Box::new(Semaphore {
            value: Mutex::new(0),
            condvar: Condvar::new(),
        }));
        // Safety: the semaphore is definitely valid for the entire lifetime of
        // the semaphore, since we have a static borrow of it.
        unsafe { pal::Semaphore::new(semaphore, None) }
    }
}
