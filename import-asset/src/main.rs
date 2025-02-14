mod cli;
mod database;
mod importers;
mod settings;

use std::{
    fs::{self, File},
    str::FromStr,
};

use anyhow::Context;
use arrayvec::ArrayString;
use cli::Command;
use database::Database;
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

    // TODO: would be nice if we could lock the file at this point if it exists,
    // to avoid overwriting changes made in between here and the write. The
    // `file_lock` feature is in FCP, so it might be possible relatively soon.
    let mut db_file = File::open(&opts.database).ok();
    let mut database = Database::new(db_file.as_mut()).expect("database file should be readable");

    process_command(&opts.command, &mut settings, &mut database)?;

    database.prune_chunks();

    info!("Writing database to: {}", opts.database.display());
    let mut db_file = File::create(&opts.database).expect("database file should be writable");
    database
        .write_into(&mut db_file)
        .expect("the modified database should be able to be written into the file");

    if original_settings != settings {
        info!("Saving new settings to: {}", opts.settings.display());
        let new_settings_str = serde_json::to_string_pretty(&settings)
            .context("Failed to serialize new import settings")?;
        fs::write(&opts.settings, new_settings_str)
            .context("Failed to write the new import settings file")?;
    }

    info!("All done! No fatal errors encountered, but check above logs to be sure.");

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

            for command in imports {
                process_command(command, settings, db)?;
            }

            if settings != &pre_reimport_settings {
                warn!("Import settings changed during reimport - check if the changes make sense.");
            }

            return Ok(());
        }

        Command::AddTexture { name, file } => {
            info!("Importing texture \"{}\" from: {}", name, file.display());
            let name = ArrayString::from_str(name).unwrap();
            let asset = importers::texture::import(file, db).context("Failed to import texture")?;
            if let Some(existing_asset) = db.textures.iter_mut().find(|a| a.name == name) {
                *existing_asset = NamedAsset { name, asset };
            } else {
                db.textures.push(NamedAsset { name, asset });
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
