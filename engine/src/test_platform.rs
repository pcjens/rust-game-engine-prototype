use core::{cell::Cell, ffi::c_void, time::Duration};

use platform_abstraction_layer::{
    ActionCategory, Button, DrawSettings, FileHandle, FileReadTask, InputDevice, InputDevices, Pal,
    PixelFormat, TextureRef, Vertex,
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

    fn begin_file_read<'a>(
        &'a self,
        file: FileHandle,
        first_byte: u64,
        buffer: &'a mut [u8],
    ) -> FileReadTask<'a> {
        FileReadTask::new(file, first_byte, buffer, self)
    }

    fn finish_file_read<'a>(&self, task: FileReadTask<'a>) -> Option<&'a mut [u8]> {
        static RESOURCES_DB: &[u8] = include_bytes!("../../resources.db");
        if task.file().inner() != 4321 {
            return None;
        }
        let first_byte = task.task_id() as usize;
        // Safety: never shared this buffer.
        let buffer = unsafe { task.into_inner() };
        buffer.copy_from_slice(&RESOURCES_DB[first_byte..first_byte + buffer.len()]);
        Some(buffer)
    }

    fn available_parallellism(&self) -> usize {
        1
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

    fn println(&self, _message: &str) {}

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
