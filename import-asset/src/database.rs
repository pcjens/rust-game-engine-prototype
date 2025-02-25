// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::{self, Cursor, Write};

use anyhow::Context;
use engine::resources::{
    audio_clip::AudioClipAsset, texture::TextureAsset, Asset, ChunkDescriptor, Deserialize,
    NamedAsset, ResourceDatabaseHeader, Serialize, TextureChunkDescriptor,
};
use tracing::{debug, trace};

#[derive(Debug)]
pub struct RelatedChunkData {
    pub chunks: Vec<ChunkDescriptor>,
    pub texture_chunks: Vec<TextureChunkDescriptor>,
    pub chunk_data: Cursor<Vec<u8>>,
}

impl RelatedChunkData {
    fn new<T: Asset>(
        asset: &mut T,
        chunk_descs: &[ChunkDescriptor],
        texchunk_descs: &[TextureChunkDescriptor],
        chunk_data: &[u8],
    ) -> RelatedChunkData {
        let mut related_chunks = Vec::new();
        let mut related_texchunks = Vec::new();
        let mut related_chunk_data = Vec::new();

        if let Some(chunk_range) = asset.get_chunks() {
            debug!("Reading chunk range: {chunk_range:?}");
            let start = related_chunks.len() as u32;
            for i in chunk_range.clone() {
                let mut desc = chunk_descs[i as usize].clone();
                let start = related_chunk_data.len() as u64;
                related_chunk_data.extend_from_slice(
                    &chunk_data[desc.source_bytes.start as usize..desc.source_bytes.end as usize],
                );
                let end = related_chunk_data.len() as u64;
                assert_eq!(desc.source_bytes.end - desc.source_bytes.start, end - start);
                desc.source_bytes = start..end;
                related_chunks.push(desc);
            }
            asset.offset_chunks(start as i32 - chunk_range.start as i32);
            debug!("Copied over {} chunks.", related_chunks.len());
        }

        if let Some(chunk_range) = asset.get_texture_chunks() {
            debug!("Reading texture chunk range: {chunk_range:?}");
            let start = related_texchunks.len() as u32;
            for i in chunk_range.clone() {
                let mut desc = texchunk_descs[i as usize].clone();
                let start = related_chunk_data.len() as u64;
                related_chunk_data.extend_from_slice(
                    &chunk_data[desc.source_bytes.start as usize..desc.source_bytes.end as usize],
                );
                let end = related_chunk_data.len() as u64;
                assert_eq!(desc.source_bytes.end - desc.source_bytes.start, end - start);
                desc.source_bytes = start..end;
                related_texchunks.push(desc);
            }
            asset.offset_chunks(start as i32 - chunk_range.start as i32);
            debug!("Copied over {} texture chunks.", related_texchunks.len());
        }

        RelatedChunkData {
            chunks: related_chunks,
            texture_chunks: related_texchunks,
            chunk_data: Cursor::new(related_chunk_data),
        }
    }

    pub fn empty() -> RelatedChunkData {
        RelatedChunkData {
            chunks: Vec::new(),
            texture_chunks: Vec::new(),
            chunk_data: Cursor::new(Vec::new()),
        }
    }
}

/// The in-memory editable version of the database, loaded on startup, written
/// back to disk at the end.
pub struct Database {
    // Asset metadata
    pub textures: Vec<(NamedAsset<TextureAsset>, RelatedChunkData)>,
    pub audio_clips: Vec<(NamedAsset<AudioClipAsset>, RelatedChunkData)>,
}

impl Database {
    pub fn new(db_file: Option<&[u8]>) -> anyhow::Result<Database> {
        if let Some(db) = db_file {
            debug!("Parsing the database.");

            let mut cursor = 0;
            let header = read_deserializable::<ResourceDatabaseHeader>(db, &mut cursor)
                .context("Failed to read resource database header")?;

            let mut chunk_descriptors = Vec::with_capacity(header.chunks as usize);
            for _ in 0..header.chunks {
                let chunk_desc = read_deserializable(db, &mut cursor)
                    .context("Failed to read chunk descriptors")?;
                chunk_descriptors.push(chunk_desc);
            }

            let mut texture_chunk_descriptors = Vec::with_capacity(header.texture_chunks as usize);
            for _ in 0..header.texture_chunks {
                let texchunk_desc = read_deserializable(db, &mut cursor)
                    .context("Failed to read texture chunk descriptors")?;
                texture_chunk_descriptors.push(texchunk_desc);
            }

            let chunk_data = &db[header.chunk_data_offset() as usize..];
            debug!(
                "The database seems to have {} bytes of chunk data.",
                chunk_data.len(),
            );

            macro_rules! read_deserializable_vec {
                ($asset_type:ty, $header:expr, $field:ident) => {{
                    let len = $header.$field as usize;
                    let mut vec = Vec::with_capacity(len);
                    debug!("Reading {} {}.", len, stringify!($field));
                    for i in 0..len {
                        let mut asset: NamedAsset<$asset_type> =
                            read_deserializable(db, &mut cursor).with_context(|| {
                                format!("Failed to read {}[{}]", stringify!($field), i)
                            })?;
                        trace!("Read {}[{}]: {:?}", stringify!($field), i, asset);
                        let related_chunk_data = RelatedChunkData::new(
                            &mut asset.asset,
                            &chunk_descriptors,
                            &texture_chunk_descriptors,
                            &chunk_data,
                        );
                        vec.push((asset, related_chunk_data));
                    }
                    vec
                }};
            }

            Ok(Database {
                textures: read_deserializable_vec!(TextureAsset, header, textures),
                audio_clips: read_deserializable_vec!(AudioClipAsset, header, audio_clips),
            })
        } else {
            Ok(Database {
                textures: Vec::new(),
                audio_clips: Vec::new(),
            })
        }
    }

    pub fn write_into(self, db_file: &mut impl Write) -> anyhow::Result<()> {
        let mut buffer = Vec::new();

        debug!("Serializing the database.");

        let mut chunk_descriptors = Vec::new();
        let mut texture_chunk_descriptors = Vec::new();
        let mut chunk_data = Vec::new();

        let mut append_chunk_data = |asset: &mut dyn Asset, asset_chunk_data: RelatedChunkData| {
            let offset = chunk_data.len();
            asset.offset_chunks(chunk_descriptors.len() as i32);
            asset.offset_texture_chunks(texture_chunk_descriptors.len() as i32);

            trace!(
                "Copying over {} chunks for this asset's range, {:?}.",
                asset_chunk_data.chunks.len(),
                asset.get_chunks(),
            );
            for chunk_desc in asset_chunk_data.chunks {
                let mut source_bytes = chunk_desc.source_bytes.clone();
                source_bytes.end += offset as u64;
                source_bytes.start += offset as u64;
                chunk_descriptors.push(ChunkDescriptor { source_bytes });
            }

            trace!(
                "Copying over {} texture chunks for this asset's range, {:?}.",
                asset_chunk_data.texture_chunks.len(),
                asset.get_texture_chunks(),
            );
            for texchunk_desc in asset_chunk_data.texture_chunks {
                let TextureChunkDescriptor {
                    region_width,
                    region_height,
                    ..
                } = texchunk_desc;
                let mut source_bytes = texchunk_desc.source_bytes.clone();
                source_bytes.end += offset as u64;
                source_bytes.start += offset as u64;
                texture_chunk_descriptors.push(TextureChunkDescriptor {
                    region_width,
                    region_height,
                    source_bytes,
                });
            }

            chunk_data.extend_from_slice(asset_chunk_data.chunk_data.get_ref());
        };

        let mut textures = (self.textures.into_iter())
            .map(|(mut asset, asset_chunk_data)| {
                append_chunk_data(&mut asset.asset, asset_chunk_data);
                asset
            })
            .collect::<Vec<_>>();
        let textures_count = textures.len();
        textures.sort();
        textures.dedup();
        assert_eq!(textures_count, textures.len());

        let mut audio_clips = (self.audio_clips.into_iter())
            .map(|(mut asset, asset_chunk_data)| {
                append_chunk_data(&mut asset.asset, asset_chunk_data);
                asset
            })
            .collect::<Vec<_>>();
        let audio_clip_count = audio_clips.len();
        audio_clips.sort();
        audio_clips.dedup();
        assert_eq!(audio_clip_count, audio_clips.len());

        let header = ResourceDatabaseHeader {
            chunks: chunk_descriptors.len() as u32,
            texture_chunks: texture_chunk_descriptors.len() as u32,
            textures: textures.len() as u32,
            audio_clips: audio_clips.len() as u32,
        };
        write_serializable(&header, &mut buffer, db_file)
            .context("Failed to write the resource database header")?;

        macro_rules! write_serializable_vec {
            ($vec:expr) => {
                debug!("Writing {}, len: {}.", stringify!($vec), $vec.len());
                for (i, serializable) in $vec.iter().enumerate() {
                    trace!("Writing {}[{}]: {:?}", stringify!($vec), i, serializable);
                    write_serializable(serializable, &mut buffer, db_file)
                        .with_context(|| format!("Failed to write {}[{}]", stringify!($vec), i))?;
                }
            };
        }

        write_serializable_vec!(&chunk_descriptors);
        write_serializable_vec!(&texture_chunk_descriptors);
        write_serializable_vec!(&textures);
        write_serializable_vec!(&audio_clips);

        debug!("Writing chunk data, {} bytes.", chunk_data.len());
        db_file
            .write_all(&chunk_data)
            .context("Failed to write the chunk data block")?;

        Ok(())
    }
}

fn write_serializable<S: Serialize>(
    serializable: &S,
    buffer: &mut Vec<u8>,
    writer: &mut impl Write,
) -> io::Result<()> {
    if S::SERIALIZED_SIZE > buffer.len() {
        let additional_needed = S::SERIALIZED_SIZE - buffer.len();
        buffer.reserve(additional_needed);
        for _ in 0..additional_needed {
            buffer.push(0);
        }
    }
    let buffer = &mut buffer[..S::SERIALIZED_SIZE];
    buffer.fill(0);
    serializable.serialize(buffer);
    writer.write_all(buffer)?;
    Ok(())
}

fn read_deserializable<D: Deserialize>(src: &[u8], cursor: &mut usize) -> io::Result<D> {
    let start = *cursor;
    let end = start + D::SERIALIZED_SIZE;
    *cursor = end;
    Ok(D::deserialize(&src[start..end]))
}
