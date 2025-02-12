mod cli;
mod database;

use std::{
    fs::{self, File},
    io::Cursor,
    str::FromStr,
};

use anyhow::Context;
use arrayvec::ArrayString;
use cli::{Command, Options};
use database::Database;
use engine::resources::{
    assets::TextureAsset, serialize, NamedAsset, ResourceDatabaseHeader, TextureChunkDescriptor,
    TEXTURE_CHUNK_FORMAT,
};
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> anyhow::Result<()> {
    let opts = cli::options().run();

    tracing_subscriber::fmt()
        .with_max_level(opts.verbosity_level)
        .finish()
        .init();

    // TODO: would be nice if we could lock the file at this point if it exists,
    // to avoid overwriting changes made in between here and the write. The
    // `file_lock` feature is in FCP, so it might be possible relatively soon.
    let mut db_file = File::open(&opts.database).ok();
    let mut database = Database::new(db_file.as_mut()).expect("database file should be readable");

    process_command(&opts, &mut database)?;

    let mut db_file = File::create(&opts.database).expect("database file should be writable");
    database
        .write_into(&mut db_file)
        .expect("the modified database should be able to be written into the file");

    return Ok(());
    // TODO: remove everything below here, keeping it here for now for reference

    #[allow(unreachable_code)]
    let mut dst = vec![0; 1_000_000];

    let mut chunk_data: Cursor<Vec<u8>> = Cursor::new(Vec::new());
    let mut texture_chunks: Vec<TextureChunkDescriptor> = Vec::new();

    let texture = {
        let image =
            image::load_from_memory(include_bytes!("../../example/resources/kellot.jpeg")).unwrap();
        let width = image.width() as u16;
        let height = image.height() as u16;
        NamedAsset {
            name: ArrayString::from_str("testing texture").unwrap(),
            asset: TextureAsset::create(
                width,
                height,
                |w, h, stride, pixels| {
                    const BPP: usize = TEXTURE_CHUNK_FORMAT.bytes_per_pixel();
                    let image = image.resize_exact(w as u32, h as u32, FilterType::CatmullRom);
                    let image = image.into_rgba8();
                    for y in 0..h as usize {
                        for x in 0..w as usize {
                            let [r, g, b, a] = image.get_pixel(x as u32, y as u32).0;
                            pixels[x * BPP + y * stride] = r;
                            pixels[x * BPP + 1 + y * stride] = g;
                            pixels[x * BPP + 2 + y * stride] = b;
                            pixels[x * BPP + 3 + y * stride] = a;
                        }
                    }
                },
                &mut chunk_data,
                &mut texture_chunks,
            ),
        }
    };

    let header = ResourceDatabaseHeader {
        chunks: 0,
        texture_chunks: texture_chunks.len() as u32,
        textures: 1,
        audio_clips: 0,
    };

    let mut cursor = 0;
    // Header
    serialize(&header, &mut dst, &mut cursor);
    // Texture chunks
    for texture_chunk in &texture_chunks {
        serialize(texture_chunk, &mut dst, &mut cursor);
    }
    // Assets
    serialize(&texture, &mut dst, &mut cursor);
    // Chunk data
    let chunk_data = chunk_data.into_inner();
    dst[cursor..cursor + chunk_data.len()].copy_from_slice(&chunk_data);
    cursor += chunk_data.len();

    fs::write("resources.db", &dst[..cursor]).unwrap();

    println!("This does not import assets yet. Wrote a resources.db for testing though.");
}

fn process_command(opts: &Options, _db: &mut Database) -> anyhow::Result<()> {
    #[derive(Serialize, Deserialize)]
    enum SettingsFile {
        V1 { imports: Vec<Command> },
    }

    let SettingsFile::V1 { imports } = if opts.settings.exists() {
        let settings = fs::read_to_string(&opts.settings)
            .context("failed to open the import settings file")?;
        serde_json::from_str(&settings).context("failed to parse the import settings file")?
    } else {
        SettingsFile::V1 {
            imports: Vec::new(),
        }
    };

    match &opts.command {
        cli::Command::Reimport {} => {
            // TODO: clean out the whole database, loop through the settings file, process each command with this function
            for _command in imports {}
        }

        cli::Command::Texture { name: _, file } => {
            let _file = File::open(file).context("failed to open texture file for importing")?;
            // TODO: read the file and engine::Serialize into a TextureAsset
            // TODO: if successful, serde::Serialize this command, add to the import settings json, write out a new version of the settings file
            todo!("texture import")
        }
    }

    Ok(())
}
