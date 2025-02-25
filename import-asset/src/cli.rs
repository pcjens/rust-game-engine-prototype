// SPDX-FileCopyrightText: 2025 Jens Pitkänen <jens.pitkanen@helsinki.fi>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::{path::PathBuf, str::FromStr};

use arrayvec::ArrayString;
use bpaf::{batteries::verbose_by_slice, Bpaf, Parser, ShellComp};
use engine::resources::ASSET_NAME_LENGTH;
use serde::{Deserialize, Serialize};
use tracing::level_filters::LevelFilter;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Bpaf)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    /// Reimports all assets in the settings file
    #[bpaf(command("reimport"))]
    Reimport {},
    /// Adds a new texture into the resource database
    #[bpaf(command("add-texture"))]
    AddTexture {
        /// The name of the texture (used to load it in game code)
        name: ArrayString<ASSET_NAME_LENGTH>,
        /// The image file to import
        #[bpaf(argument("FILE"), complete_shell(ShellComp::File { mask: None }))]
        file: PathBuf,
    },
    /// Adds a new audio clip into the resource database
    #[bpaf(command("add-audio"))]
    AddAudioClip {
        /// The name of the audio clip (used to load it in game code)
        name: ArrayString<ASSET_NAME_LENGTH>,
        /// The audio file to import
        #[bpaf(argument("FILE"), complete_shell(ShellComp::File { mask: None }))]
        file: PathBuf,
        /// The track number to import from the audio file, with the first track
        /// being number 0 (defaults to a format-dependent "default track")
        #[bpaf(argument("NUMBER"))]
        track: Option<usize>,
    },
}

impl Command {
    /// Returns the name of the asset this command imports. Used to match up two
    /// commands operating on the same asset.
    pub fn asset_name(&self) -> Option<&str> {
        match self {
            Command::Reimport {} => None,
            Command::AddTexture { name, .. } => Some(name),
            Command::AddAudioClip { name, .. } => Some(name),
        }
    }
}

/// Asset importer for the engine. Without any arguments, simply reimports all
/// assets in the import-settings.json file, and writes out the database file
/// into resources.db.
#[derive(Debug, Clone, Bpaf)]
#[bpaf(options)]
pub struct Options {
    #[bpaf(external(verbosity_parser))]
    pub verbosity_level: LevelFilter,
    /// Selects the resource database file to overwrite (default: resources.db)
    #[bpaf(
        argument("FILE"), 
        fallback_with(|| PathBuf::from_str("resources.db")), 
        complete_shell(ShellComp::File { mask: Some("*.db") }),
    )]
    pub database: PathBuf,
    /// Selects the import settings file to use (default: import-settings.json)
    #[bpaf(
        argument("FILE"), 
        fallback_with(|| PathBuf::from_str("import-settings.json")), 
        complete_shell(ShellComp::File { mask: Some("*.json") }),
    )]
    pub settings: PathBuf,
    #[bpaf(external)]
    pub command: Command,
}

fn verbosity_parser() -> impl Parser<LevelFilter> {
    verbose_by_slice(
        3,
        [
            LevelFilter::OFF,
            LevelFilter::ERROR,
            LevelFilter::WARN,
            LevelFilter::INFO,
            LevelFilter::DEBUG,
            LevelFilter::TRACE,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::options;

    #[test]
    fn check_bpaf_invariants() {
        options().check_invariants(true);
    }
}
