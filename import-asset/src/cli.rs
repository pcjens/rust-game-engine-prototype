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
    /// Adds a newtexture into the resource database and saves the settings
    #[bpaf(command("add-texture"))]
    AddTexture {
        /// The name of the texture (used to load it in game code)
        #[bpaf(complete_shell(ShellComp::File { mask: None }))]
        name: ArrayString<ASSET_NAME_LENGTH>,
        /// The image file to import as a texture
        #[bpaf(complete_shell(ShellComp::File { mask: None }))]
        file: PathBuf,
    },
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
