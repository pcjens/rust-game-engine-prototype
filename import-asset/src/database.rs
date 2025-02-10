use std::io::{self, Read, Write};

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
    pub chunk_data: Vec<u8>,
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
                    chunk_data
                },
            })
        } else {
            Ok(Database {
                chunk_descriptors: Vec::new(),
                texture_chunk_descriptors: Vec::new(),
                textures: Vec::new(),
                audio_clips: Vec::new(),
                chunk_data: Vec::with_capacity(CHUNK_SIZE as usize),
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

        debug!("writing chunk data, {} bytes", self.chunk_data.len());
        db_file
            .write_all(&self.chunk_data)
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
