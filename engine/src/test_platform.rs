extern crate alloc;

use core::ffi::c_void;

use alloc::vec::Vec;
use platform_abstraction_layer::{
    ActionCategory, Button, DrawSettings, InputDevice, InputDevices, Pal, PixelFormat, TextureRef,
    Vertex,
};

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct VeryAlignedThing([u8; 64]);
const VERY_ALIGNED_THING: VeryAlignedThing = VeryAlignedThing([0; 64]);

pub struct TestPlatform;

impl TestPlatform {
    pub fn new() -> TestPlatform {
        TestPlatform
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

    fn input_devices(&self) -> InputDevices {
        InputDevices::new()
    }

    fn default_button_for_action(
        &self,
        _action: ActionCategory,
        _device: InputDevice,
    ) -> Option<Button> {
        None
    }

    fn println(&self, _message: &str) {}

    fn exit(&self, clean: bool) {
        if !clean {
            panic!("TestPlatform::exit({clean}) was called (test ran into an error?)");
        }
    }

    fn malloc(&self, size: usize) -> *mut c_void {
        let count = size.div_ceil(size_of::<VeryAlignedThing>());
        let byte_vec: Vec<VeryAlignedThing> = alloc::vec![VERY_ALIGNED_THING; count];
        let vec_ptr: *mut VeryAlignedThing = byte_vec.leak().as_mut_ptr();
        vec_ptr as *mut c_void
    }

    unsafe fn free(&self, ptr: *mut c_void, size: usize) {
        self.free_impl(ptr, size);
    }
}

impl TestPlatform {
    fn free_impl(&self, ptr: *mut c_void, size: usize) {
        let vec_ptr = ptr as *mut VeryAlignedThing;
        let count = size.div_ceil(size_of::<VeryAlignedThing>());
        // Safety: ptr was allocated by a Vec<u8> so the requirements are
        // upheld, based on Vec::from_raw_parts documentation. The length and
        // capacity also match the original Vec.
        let byte_vec: Vec<VeryAlignedThing> = unsafe { Vec::from_raw_parts(vec_ptr, count, count) };
        drop(byte_vec);
    }
}
