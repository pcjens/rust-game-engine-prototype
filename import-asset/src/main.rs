// SPDX-FileCopyrightText: 2024 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod database;
mod importers;
mod settings;

use std::{
    fs::{self, File},
    io::BufWriter,
    str::FromStr,
};

use anyhow::Context;
use arrayvec::ArrayString;
use cli::Command;
use database::{Database, RelatedChunkData};
use engine::resources::NamedAsset;
use settings::ImportSettings;
use tracing::{info, warn};
use tracing_subscriber::util::SubscriberInitExt;

fn main() -> anyhow::Result<()> {
    let opts = cli::options().run();

    tracing_subscriber::fmt()
        .with_max_level(opts.verbosity_level)
        .finish()
        .init();

    let mut settings = settings::read(&opts.settings)?;
    let original_settings = settings.clone();

    // TODO: lock the file at this point if it exists, to avoid overwriting
    // changes made in between here and the write. The Rust standard library
    // `file_lock` feature is in FCP, so it might be possible relatively soon.
    info!("Reading database from: {}", opts.database.display());
    let db_file = fs::read(&opts.database).ok();
    let mut database = Database::new(db_file.as_deref()).context("Failed to read database file")?;

    process_command(&opts.command, &mut settings, &mut database)?;

    info!("Writing database to: {}", opts.database.display());
    let mut db_file = BufWriter::new(
        File::create(&opts.database).context("Failed to open database file for writing")?,
    );
    database
        .write_into(&mut db_file)
        .context("Failed to write the database back into the file")?;

    if original_settings != settings {
        info!("Saving new settings to: {}", opts.settings.display());
        let new_settings_str = serde_json::to_string_pretty(&settings)
            .context("Failed to serialize new import settings")?;
        fs::write(&opts.settings, new_settings_str)
            .context("Failed to write the new import settings file")?;
    }

    info!("All done! No fatal errors, but check the logs above for less severe issues.");

    Ok(())
}

fn process_command(
    command: &Command,
    settings: &mut ImportSettings,
    db: &mut Database,
) -> anyhow::Result<()> {
    let ImportSettings::V1 { imports } = settings;

    match command {
        Command::Reimport {} => {
            let pre_reimport_settings = settings.clone();
            let ImportSettings::V1 { imports } = &pre_reimport_settings;

            info!("Reimporting {} assets.", imports.len());

            db.clear();
            for command in imports {
                process_command(command, settings, db)?;
            }

            if settings != &pre_reimport_settings {
                warn!("Import settings changed during reimport - check if the changes make sense.");
            }

            return Ok(());
        }

        Command::AddSprite { name, file } => {
            info!("Importing sprite \"{}\" from: {}", name, file.display());
            let mut related_chunk_data = RelatedChunkData::empty();
            let name = ArrayString::from_str(name).unwrap();
            let asset = importers::sprite::import(file, &mut related_chunk_data)
                .context("Failed to import sprite")?;
            let asset_and_data = (NamedAsset { name, asset }, related_chunk_data);
            if let Some(existing_asset) = db.sprites.iter_mut().find(|a| a.0.name == name) {
                *existing_asset = asset_and_data;
            } else {
                db.sprites.push(asset_and_data);
            }
        }

        Command::AddAudioClip { name, file, track } => {
            info!("Importing audio clip \"{}\" from: {}", name, file.display());
            let mut related_chunk_data = RelatedChunkData::empty();
            let name = ArrayString::from_str(name).unwrap();
            let asset = importers::audio_clip::import(file, *track, &mut related_chunk_data)
                .context("Failed to import audio clip")?;
            let asset_and_data = (NamedAsset { name, asset }, related_chunk_data);
            if let Some(existing_asset) = db.audio_clips.iter_mut().find(|a| a.0.name == name) {
                *existing_asset = asset_and_data;
            } else {
                db.audio_clips.push(asset_and_data);
            }
        }
    }

    // In case the command operated on an asset, update the command in the import settings.
    if let Some(name) = command.asset_name() {
        if let Some(import) = imports.iter_mut().find(|c| c.asset_name() == Some(name)) {
            *import = command.clone();
        } else {
            imports.push(command.clone());
        }
    }

    Ok(())
}
