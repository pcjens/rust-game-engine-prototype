// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{
    fs::File,
    io::{ErrorKind, Seek, Write},
    path::Path,
};

use anyhow::Context;
use engine::resources::{audio_clip::AudioClipAsset, ChunkDescriptor, CHUNK_SIZE};
use platform::{AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};
use symphonia::{
    core::{
        audio::{AudioBuffer, Channels, Signal, SignalSpec},
        codecs::DecoderOptions,
        errors::Error as SymphoniaError,
        formats::FormatOptions,
        io::{MediaSourceStream, MediaSourceStreamOptions},
        meta::MetadataOptions,
        probe::Hint,
    },
    default,
};
use tracing::{debug, trace};

use crate::database::RelatedChunkData;

const SAMPLES_PER_CHUNK: usize = CHUNK_SIZE as usize / size_of::<i16>();

pub fn import(
    audio_path: &Path,
    track: Option<usize>,
    db: &mut RelatedChunkData,
) -> anyhow::Result<AudioClipAsset> {
    let samples = read_audio_file(audio_path, track).context("Failed to read the audio file")?;

    let chunk_start = db.chunks.len() as u32;
    for samples_chunk in samples.chunks(SAMPLES_PER_CHUNK) {
        let chunk_data_start = db.chunk_data.stream_position().unwrap();
        db.chunk_data
            .write_all(bytemuck::cast_slice(samples_chunk))
            .unwrap();
        let chunk_data_end = db.chunk_data.stream_position().unwrap();
        debug!(
            "Writing {} audio samples ({}..{}) to chunk {}.",
            samples_chunk.len(),
            chunk_data_start,
            chunk_data_end,
            db.chunks.len()
        );
        db.chunks.push(ChunkDescriptor {
            source_bytes: chunk_data_start..chunk_data_end,
        });
    }
    let chunk_end = db.chunks.len() as u32;
    debug!(
        "Created {} chunks ({}..{}) for audio clip asset from {}.",
        chunk_end - chunk_start,
        chunk_start,
        chunk_end,
        audio_path.display(),
    );

    Ok(AudioClipAsset {
        samples: samples.len() as u32,
        chunks: chunk_start..chunk_end,
    })
}

fn read_audio_file(
    path: &Path,
    track: Option<usize>,
) -> anyhow::Result<Vec<[i16; AUDIO_CHANNELS]>> {
    debug!("Reading audio data from: {}", path.display());

    let codecs = default::get_codecs();
    let probe = default::get_probe();

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().map(|s| s.to_string_lossy()) {
        hint.with_extension(&extension);
    }

    let file = File::open(path).context("Could not open audio file for reading")?;
    let source = MediaSourceStream::new(Box::new(file), MediaSourceStreamOptions::default());
    let mut source = probe
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("Could not recognize audio format")?;

    let track_count = source.format.tracks().len();
    if track_count == 0 {
        return Err(anyhow::anyhow!(
            "The file appears to be an audio file, but without any tracks?"
        ));
    }
    let track = if let Some(track) = track {
        if track >= track_count {
            return Err(anyhow::anyhow!("Track number {track} wasn't found in this audio file, it only has {track_count} tracks. Note the numbering starts at 0."));
        }
        &source.format.tracks()[track]
    } else {
        source.format.default_track().unwrap()
    };

    let mut decoder = codecs
        .make(&track.codec_params, &DecoderOptions::default())
        .context("Failed to create a decoder for the audio")?;

    let mut samples = Vec::new();
    loop {
        let packet = match source.format.next_packet() {
            Ok(packet) => packet,

            // This seems to signal that we're done, as "end of stream" means
            // "read finished" according to the docs, but there doesn't seem to
            // be a properly typed end of stream error.
            Err(SymphoniaError::IoError(err)) if err.kind() == ErrorKind::UnexpectedEof => break,

            // These are recoverable according to Decode::decoder docs.
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::ResetRequired) => {
                samples.clear();
                decoder = codecs
                    .make(decoder.codec_params(), &DecoderOptions::default())
                    .context("Failed to recreate a decoder for the audio")?;
                continue;
            }

            Err(err) => Err(err).context("Failed to read audio data packet")?,
        };

        let decoded = decoder
            .decode(&packet)
            .context("Failed to decode audio data packet")?;

        trace!(
            "Decoded audio data, {} frames of: {:?}",
            decoded.frames(),
            decoded.spec(),
        );

        assert_eq!(
            2, AUDIO_CHANNELS,
            "this conversion step assumes simple stereo audio buffers",
        );
        let mut converted = AudioBuffer::<i16>::new(
            decoded.capacity() as u64,
            SignalSpec {
                rate: AUDIO_SAMPLE_RATE,
                channels: Channels::FRONT_LEFT | Channels::FRONT_RIGHT,
            },
        );
        decoded.convert(&mut converted);
        trace!("Converted audio to a engine-native signal spec.");

        samples.reserve(converted.frames());
        for (&left, &right) in converted.chan(0).iter().zip(converted.chan(1)) {
            samples.push([left, right]);
        }
    }

    Ok(samples)
}
