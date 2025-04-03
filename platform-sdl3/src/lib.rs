// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use platform::{EngineCallbacks, Platform};
use sdl3_main::{AppResultWithState, state::SyncPtr};
pub use sdl3_sys::events::SDL_Event;

#[macro_export]
macro_rules! define_sdl3_main {
    ($engine_type:ty, $game_type:ty, $init_fn:ident, $iterate_fn:ident) => {
        use sdl3_main::state::SyncPtr;

        type AppState = $crate::Sdl3Platform<$engine_type, $game_type>;

        #[sdl3_main::app_init]
        fn app_init() -> sdl3_main::AppResultWithState<SyncPtr<AppState>> {
            $crate::app_init::<$engine_type, $game_type>()
        }

        #[sdl3_main::app_event]
        fn app_event(state: SyncPtr<AppState>, event: &platform_sdl3::SDL_Event) {
            todo!()
        }

        #[sdl3_main::app_iterate]
        fn app_iterate(state: SyncPtr<AppState>) -> sdl3_main::AppResult {
            todo!()
        }

        #[sdl3_main::app_quit]
        fn app_quit() {}
    };
}

pub struct Sdl3Platform<E: Sync + EngineCallbacks, G: Sync> {
    engine: E,
    game: G,
}

pub fn app_init<E: Sync + EngineCallbacks, G: Sync>()
-> AppResultWithState<SyncPtr<Sdl3Platform<E, G>>> {
    todo!()
}

impl<E: Sync + EngineCallbacks, G: Sync> Platform for Sdl3Platform<E, G> {
    fn draw_area(&self) -> (f32, f32) {
        todo!()
    }

    fn draw_scale_factor(&self) -> f32 {
        todo!()
    }

    fn draw_2d(
        &self,
        _vertices: &[platform::Vertex2D],
        _indices: &[u32],
        _settings: platform::DrawSettings2D,
    ) {
        todo!()
    }

    fn create_sprite(
        &self,
        _width: u16,
        _height: u16,
        _format: platform::PixelFormat,
    ) -> Option<platform::SpriteRef> {
        todo!()
    }

    fn update_sprite(
        &self,
        _sprite: platform::SpriteRef,
        _x_offset: u16,
        _y_offset: u16,
        _width: u16,
        _height: u16,
        _pixels: &[u8],
    ) {
        todo!()
    }

    fn open_file(&self, _path: &str) -> Option<platform::FileHandle> {
        todo!()
    }

    fn begin_file_read(
        &self,
        _file: platform::FileHandle,
        _first_byte: u64,
        _buffer: platform::Box<[u8]>,
    ) -> platform::FileReadTask {
        todo!()
    }

    fn is_file_read_finished(&self, _task: &platform::FileReadTask) -> bool {
        todo!()
    }

    fn finish_file_read(
        &self,
        _task: platform::FileReadTask,
    ) -> Result<platform::Box<[u8]>, platform::Box<[u8]>> {
        todo!()
    }

    fn create_semaphore(&self) -> platform::Semaphore {
        todo!()
    }

    fn available_parallelism(&self) -> usize {
        todo!()
    }

    fn spawn_pool_thread(&self, _channels: [platform::TaskChannel; 2]) -> platform::ThreadState {
        todo!()
    }

    fn update_audio_buffer(
        &self,
        _first_position: u64,
        _samples: &[[i16; platform::AUDIO_CHANNELS]],
    ) {
        todo!()
    }

    fn audio_playback_position(&self) -> (u64, platform::Instant) {
        todo!()
    }

    fn input_devices(&self) -> platform::InputDevices {
        todo!()
    }

    fn default_button_for_action(
        &self,
        _action: platform::ActionCategory,
        _device: platform::InputDevice,
    ) -> Option<platform::Button> {
        todo!()
    }

    fn now(&self) -> platform::Instant {
        todo!()
    }

    fn println(&self, _message: std::fmt::Arguments) {
        todo!()
    }

    fn exit(&self, _clean: bool) {
        todo!()
    }
}
