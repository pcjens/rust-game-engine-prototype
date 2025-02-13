use std::{
    io::{self, Cursor, Read, Write},
    ops::Range,
};

use anyhow::Context;
use engine::resources::{
    assets::{AudioClipAsset, TextureAsset},
    ChunkDescriptor, Deserialize, NamedAsset, ResourceDatabaseHeader, Serialize,
    TextureChunkDescriptor, CHUNK_SIZE,
};
use tracing::{debug, info, trace};

/// The in-memory editable version of the database, loaded on startup, written
/// back to disk at the end.
pub struct Database {
    // Chunk loading metadata
    pub chunk_descriptors: Vec<ChunkDescriptor>,
    pub texture_chunk_descriptors: Vec<TextureChunkDescriptor>,
    // Asset metadata
    pub textures: Vec<NamedAsset<TextureAsset>>,
    pub audio_clips: Vec<NamedAsset<AudioClipAsset>>,
    // Chunk data region
    pub chunk_data: Cursor<Vec<u8>>,
}

impl Database {
    pub fn new(db_file: Option<&mut impl Read>) -> anyhow::Result<Database> {
        if let Some(db_file) = db_file {
            let mut buffer = Vec::new();

            info!("Reading database file.");

            let header = read_deserializable::<ResourceDatabaseHeader>(&mut buffer, db_file)
                .context("Failed to read resource database header")?;

            macro_rules! read_vec {
                ($header:expr, $field:ident, $buffer:expr, $reader:expr) => {{
                    let len = $header.$field as usize;
                    let mut vec = Vec::with_capacity(len);
                    debug!("reading {} {}", len, stringify!($field));
                    for i in 0..len {
                        vec.push(read_deserializable($buffer, $reader).with_context(|| {
                            format!("Failed to read {}[{}]", stringify!($field), i)
                        })?);
                        trace!("read {}[{}]: {:?}", stringify!($field), i, &vec[i]);
                    }
                    vec
                }};
            }

            Ok(Database {
                chunk_descriptors: read_vec!(header, chunks, &mut buffer, db_file),
                texture_chunk_descriptors: read_vec!(header, texture_chunks, &mut buffer, db_file),
                textures: read_vec!(header, textures, &mut buffer, db_file),
                audio_clips: read_vec!(header, audio_clips, &mut buffer, db_file),
                chunk_data: {
                    let mut chunk_data = Vec::new();
                    db_file
                        .read_to_end(&mut chunk_data)
                        .context("Failed to read the chunk data block")?;
                    debug!("read {} bytes of chunk data", chunk_data.len());
                    let chunk_len = chunk_data.len();
                    let mut cursor = Cursor::new(chunk_data);
                    cursor.set_position(chunk_len as u64);
                    cursor
                },
            })
        } else {
            Ok(Database {
                chunk_descriptors: Vec::new(),
                texture_chunk_descriptors: Vec::new(),
                textures: Vec::new(),
                audio_clips: Vec::new(),
                chunk_data: Cursor::new(Vec::with_capacity(CHUNK_SIZE as usize)),
            })
        }
    }

    pub fn write_into(self, db_file: &mut impl Write) -> anyhow::Result<()> {
        let mut buffer = Vec::new();

        info!("Writing database file.");

        let header = ResourceDatabaseHeader {
            chunks: self.chunk_descriptors.len() as u32,
            texture_chunks: self.texture_chunk_descriptors.len() as u32,
            textures: self.textures.len() as u32,
            audio_clips: self.audio_clips.len() as u32,
        };
        write_serializable(&header, &mut buffer, db_file)
            .context("Failed to write the resource database header")?;

        macro_rules! write_vec {
            ($vec:expr, $buffer:expr, $writer:expr) => {
                debug!("writing {}, len: {}", stringify!($vec), $vec.len());
                for (i, serializable) in $vec.iter().enumerate() {
                    trace!("writing {}[{}]: {:?}", stringify!($vec), i, serializable);
                    write_serializable(serializable, $buffer, $writer)
                        .with_context(|| format!("Failed to write {}[{}]", stringify!($vec), i))?;
                }
            };
        }

        write_vec!(&self.chunk_descriptors, &mut buffer, db_file);
        write_vec!(&self.texture_chunk_descriptors, &mut buffer, db_file);
        write_vec!(&self.textures, &mut buffer, db_file);
        write_vec!(&self.audio_clips, &mut buffer, db_file);

        let chunk_data = self.chunk_data.into_inner();
        debug!("writing chunk data, {} bytes", chunk_data.len());
        db_file
            .write_all(&chunk_data)
            .context("Failed to write the chunk data block")?;

        Ok(())
    }

    /// Removes any unused chunks and the data they point to.
    ///
    /// Implementation detail: this function recreates all of the chunks, which
    /// may result in chunk data being reordered.
    pub fn prune_chunks(&mut self) {
        let old_chunks = &self.chunk_descriptors;
        let old_texchunks = &self.texture_chunk_descriptors;
        let old_chunk_data = self.chunk_data.get_ref();

        // FIMXE: Chunks reused between multiple assets get cloned for each asset currently

        let mut new_chunks = Vec::with_capacity(old_chunks.len());
        let mut new_texchunks = Vec::with_capacity(old_texchunks.len());
        let mut new_chunk_data = Vec::with_capacity(old_chunk_data.len());

        let mut add_chunks = |old_chunk_range: Range<u32>| -> Range<u32> {
            let start = new_chunks.len() as u32;
            for i in old_chunk_range {
                let ChunkDescriptor {
                    source_bytes: Range { start, end },
                } = old_chunks[i as usize];

                let new_start = new_chunk_data.len() as u64;
                new_chunk_data.extend_from_slice(&old_chunk_data[start as usize..end as usize]);
                let new_end = new_chunk_data.len() as u64;

                new_chunks.push(ChunkDescriptor {
                    source_bytes: new_start..new_end,
                });
            }
            let end = new_texchunks.len() as u32;
            start..end
        };

        for audio_clip in &mut self.audio_clips {
            audio_clip.asset.chunks = add_chunks(audio_clip.asset.chunks.clone());
        }

        let mut add_texchunks = |old_chunk_range: Range<u32>| -> Range<u32> {
            let start = new_texchunks.len() as u32;
            for i in old_chunk_range {
                let TextureChunkDescriptor {
                    region_width,
                    region_height,
                    source_bytes: Range { start, end },
                } = old_texchunks[i as usize];

                let new_start = new_chunk_data.len() as u64;
                new_chunk_data.extend_from_slice(&old_chunk_data[start as usize..end as usize]);
                let new_end = new_chunk_data.len() as u64;

                new_texchunks.push(TextureChunkDescriptor {
                    region_width,
                    region_height,
                    source_bytes: new_start..new_end,
                });
            }
            let end = new_texchunks.len() as u32;
            start..end
        };

        for texture in &mut self.textures {
            for mip_level in &mut texture.asset.mip_chain {
                match mip_level {
                    engine::resources::assets::TextureMipLevel::SingleChunkTexture {
                        texture_chunk,
                        ..
                    } => {
                        let old_chunks = *texture_chunk..*texture_chunk + 1;
                        let new_chunks = add_texchunks(old_chunks);
                        assert_eq!(new_chunks.start + 1, new_chunks.end);
                        *texture_chunk = new_chunks.start;
                    }
                    engine::resources::assets::TextureMipLevel::MultiChunkTexture {
                        texture_chunks,
                        ..
                    } => *texture_chunks = add_texchunks(texture_chunks.clone()),
                }
            }
        }

        self.chunk_descriptors = new_chunks;
        self.texture_chunk_descriptors = new_texchunks;
        self.chunk_data = Cursor::new(new_chunk_data);
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

fn read_deserializable<D: Deserialize>(
    buffer: &mut Vec<u8>,
    reader: &mut impl Read,
) -> io::Result<D> {
    if D::SERIALIZED_SIZE > buffer.len() {
        let additional_needed = D::SERIALIZED_SIZE - buffer.len();
        buffer.reserve(additional_needed);
        for _ in 0..additional_needed {
            buffer.push(0);
        }
    }
    let buffer = &mut buffer[..D::SERIALIZED_SIZE];
    reader.read_exact(buffer)?;
    Ok(D::deserialize(buffer))
}
