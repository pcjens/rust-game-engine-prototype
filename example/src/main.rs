#[cfg(feature = "platform-sdl2")]
platform_sdl2::generate_main!();

#[cfg(not(any(feature = "platform-sdl2")))]
fn main() {
    compile_error!("at least one of the following platform features is required: 'platform-sdl2'");
}
