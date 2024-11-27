#[cfg(feature = "platform-sdl2")]
fn main() {
    platform_sdl2::main_impl();
}

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
