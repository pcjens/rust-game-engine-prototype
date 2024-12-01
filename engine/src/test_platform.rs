extern crate alloc;

use core::ffi::c_void;

use alloc::vec::Vec;
use pal::Pal;

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct VeryAlignedThing([u8; 64]);
const VERY_ALIGNED_THING: VeryAlignedThing = VeryAlignedThing([0; 64]);

#[derive(Default)]
pub struct TestPlatform;

impl Pal for TestPlatform {
    fn println(&self, _message: &str) {}

    fn exit(&self, clean: bool) -> ! {
        panic!("TestPlatform::exit({clean}) was called");
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
        // Safety: ptr was allocated by a Vec<u8> so the requirements are
        // upheld, based on Vec::from_raw_parts documentation. The length and
        // capacity also match the original Vec.
        let byte_vec: Vec<VeryAlignedThing> = unsafe { Vec::from_raw_parts(vec_ptr, size, size) };
        drop(byte_vec);
    }
}
