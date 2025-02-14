use std::{fs, path::Path};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::cli::Command;

/// The import settings file containing the settings passed into past
/// import-asset invocations.
///
/// Used to keep track of assets the database contains alongside any import-time
/// configurations.
///
/// Has enum variants for breaking changes in the format of the settings file,
/// but [`read`] always returns the newest variant.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "settings_file_version", rename_all = "snake_case")]
pub enum ImportSettings {
    V1 { imports: Vec<Command> },
}

pub fn read(settings: &Path) -> anyhow::Result<ImportSettings> {
    let settings = if settings.exists() {
        let settings =
            fs::read_to_string(settings).context("Failed to open the import settings file")?;
        serde_json::from_str(&settings).context("Failed to parse the import settings file")?
    } else {
        ImportSettings::V1 {
            imports: Vec::new(),
        }
    };

    // NOTE: When there's new versions of SettingsFile, convert to the newest
    // here (process_command assumes it)

    Ok(settings)
}
