// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use core::{cmp::Reverse, time::Duration};

use platform::{thread_pool::ThreadPool, Platform, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

use crate::{
    allocators::LinearAllocator,
    collections::FixedVec,
    resources::{
        audio_clip::AudioClipHandle, ResourceDatabase, ResourceLoader, AUDIO_SAMPLES_PER_CHUNK,
    },
};

#[derive(Debug)]
struct PlayingClip {
    channel: usize,
    clip: AudioClipHandle,
    start_position: u64,
}

impl PlayingClip {
    fn get_end(&self, resources: &ResourceDatabase) -> u64 {
        self.start_position + resources.get_audio_clip(self.clip).samples as u64
    }
}

#[derive(Debug)]
struct ChannelSettings {
    volume: u8,
}

/// Holds currently playing audio tracks and their playback parameters.
pub struct Mixer {
    playing_clips: FixedVec<'static, PlayingClip>,
    channels: FixedVec<'static, ChannelSettings>,
    playback_buffer: FixedVec<'static, [i16; AUDIO_CHANNELS]>,
    /// The audio position where new sounds should start playing, updated at the
    /// start of each frame with [`Mixer::update_audio_sync`].
    audio_position: u64,
}

impl Mixer {
    /// Creates a new [`Mixer`] with the specified amount of channels, a cap for
    /// how many sounds can play at the same time, and a buffer length (in
    /// samples), returning None if the allocator doesn't have enough memory.
    ///
    /// Each channel has its own set of controllable parameters, for e.g. tuning
    /// the volume between music and sound effects separately.
    ///
    /// The playback buffer's length should be at least as long as the
    /// platform's audio buffer, plus how many samples would be played back
    /// during one frame, to avoid choppy audio. A longer length will help with
    /// avoiding audio cutting out in case of lagspikes, at the cost of taking
    /// up more memory and slowing down [`Mixer::render_audio`]. This buffer
    /// length does not affect latency.
    pub fn new(
        arena: &'static LinearAllocator,
        channel_count: usize,
        max_playing_clips: usize,
        playback_buffer_length: usize,
    ) -> Option<Mixer> {
        let mut playback_buffer = FixedVec::new(arena, playback_buffer_length)?;
        playback_buffer.fill_with_zeroes();

        let playing_clips = FixedVec::new(arena, max_playing_clips)?;

        let mut channels = FixedVec::new(arena, channel_count)?;
        for _ in 0..channel_count {
            channels.push(ChannelSettings { volume: 0xFF }).unwrap();
        }

        Some(Mixer {
            playing_clips,
            channels,
            playback_buffer,
            audio_position: 0,
        })
    }

    /// Plays the audio clip starting this frame, returning false if the sound
    /// can't be played.
    ///
    /// If the mixer is already playing the maximum amount of concurrent clips,
    /// and `important` is `true`, the clip with the least playback time left
    /// will be replaced with this sound. Note that this may cause popping audio
    /// artifacts, though on the other hand, with many other sounds playing, it
    /// may not be as noticeable. If `important` is `false`, this sound will not
    /// be played.
    ///
    /// If the channel index is out of bounds, the sound will not be played.
    pub fn play_clip(
        &mut self,
        channel: usize,
        clip: AudioClipHandle,
        important: bool,
        resources: &ResourceDatabase,
    ) -> bool {
        if channel >= self.channels.len() {
            return false;
        }

        let playing_clip = PlayingClip {
            channel,
            clip,
            start_position: self.audio_position,
        };

        if !self.playing_clips.is_full() {
            self.playing_clips.push(playing_clip).unwrap();
        } else if important {
            if self.playing_clips.is_empty() {
                return false;
            }

            let mut lowest_end_time = self.playing_clips[0].get_end(resources);
            let mut candidate_index = 0;
            for (i, clip) in self.playing_clips.iter().enumerate().skip(1) {
                let end_time = clip.get_end(resources);
                if end_time < lowest_end_time {
                    lowest_end_time = end_time;
                    candidate_index = i;
                }
            }

            self.playing_clips[candidate_index] = playing_clip;
        } else {
            return false;
        }

        true
    }

    /// Synchronizes the mixer's internal clock with the platform's audio
    /// buffer.
    ///
    /// Should be called at the start of the frame by the engine.
    pub fn update_audio_sync(&mut self, frame_elapsed: Duration, platform: &dyn Platform) {
        let (playback_pos, playback_elapsed) = platform.audio_playback_position();
        if let Some(time_since_playback_pos) = frame_elapsed.checked_sub(playback_elapsed) {
            let frame_offset_from_playback_pos =
                time_since_playback_pos.as_micros() * AUDIO_SAMPLE_RATE as u128 / 1_000_000;
            self.audio_position = playback_pos + frame_offset_from_playback_pos as u64;
        } else {
            self.audio_position = playback_pos;
        }
    }

    /// Mixes the currently playing tracks together and updates the platform's
    /// audio buffer with the result.
    ///
    /// Should be called at the end of the frame by the engine.
    pub fn render_audio(
        &mut self,
        _thread_pool: &mut ThreadPool,
        platform: &dyn Platform,
        resources: &ResourceDatabase,
        resource_loader: &mut ResourceLoader,
    ) {
        let (playback_start, _) = platform.audio_playback_position();

        // Remove clips that have played to the end
        self.playing_clips
            .sort_unstable_by_key(|clip| Reverse(clip.get_end(resources)));
        if let Some(finished_clips_start_index) = (self.playing_clips)
            .iter()
            .position(|clip| clip.get_end(resources) < playback_start)
        {
            self.playing_clips.truncate(finished_clips_start_index);
        }

        // Render
        self.playback_buffer.fill([0; AUDIO_CHANNELS]);
        // TODO: use parallellize() here once it allows borrowing
        for clip in &*self.playing_clips {
            let volume = self.channels[clip.channel].volume;
            let asset = resources.get_audio_clip(clip.clip);

            let skipped = playback_start.saturating_sub(clip.start_position) as u32;
            let first_chunk = asset.chunks.start + skipped / AUDIO_SAMPLES_PER_CHUNK as u32;
            let last_chunk = asset.chunks.start + asset.samples / AUDIO_SAMPLES_PER_CHUNK as u32;

            let mut playback_offset = clip.start_position.saturating_sub(playback_start) as usize;
            for chunk_index in first_chunk..=last_chunk {
                if self.playback_buffer.len() <= playback_offset {
                    break;
                }

                let chunk_start = chunk_index * AUDIO_SAMPLES_PER_CHUNK as u32;
                let chunk_end = (chunk_index + 1) * AUDIO_SAMPLES_PER_CHUNK as u32;

                if let Some(chunk) = &resources.chunks.get(chunk_index) {
                    let chunk_samples = bytemuck::cast_slice::<u8, [i16; AUDIO_CHANNELS]>(&chunk.0);
                    let first_sample_idx = (skipped.max(chunk_start) - chunk_start) as usize;
                    let last_sample_idx = (asset.samples.min(chunk_end) - chunk_start) as usize;
                    render_audio_chunk(
                        &chunk_samples[first_sample_idx..last_sample_idx],
                        &mut self.playback_buffer[playback_offset..],
                        volume,
                    );
                    playback_offset += last_sample_idx - first_sample_idx;
                } else {
                    break;
                }
            }
        }

        // Send the rendered audio to be played back
        platform.update_audio_buffer(playback_start, &self.playback_buffer);

        // Queue up any missing audio chunks in preparation for the next frame
        for clip in &*self.playing_clips {
            let asset = resources.get_audio_clip(clip.clip);
            let current_pos = playback_start.saturating_sub(clip.start_position);
            let current_chunk_index = (current_pos / AUDIO_SAMPLES_PER_CHUNK as u64) as u32;
            let next_chunk_index = current_chunk_index + 1;

            resource_loader.queue_chunk(asset.chunks.start + current_chunk_index, resources);
            if asset.chunks.start + next_chunk_index < asset.chunks.end {
                resource_loader.queue_chunk(asset.chunks.start + next_chunk_index, resources);
            }
        }
    }
}

fn render_audio_chunk(
    chunk_samples: &[[i16; AUDIO_CHANNELS]],
    dst: &mut [[i16; AUDIO_CHANNELS]],
    volume: u8,
) {
    for (dst, sample) in dst.iter_mut().zip(chunk_samples) {
        for channel in 0..AUDIO_CHANNELS {
            let sample = sample[channel];
            let attenuated = ((sample as i32 * volume as i32) / u8::MAX as i32) as i16;
            dst[channel] += attenuated;
        }
    }
}
