use std::path::PathBuf;

use bpaf::{batteries::verbose_by_slice, construct, long, OptionParser, Parser};
use tracing::level_filters::LevelFilter;

#[derive(Debug, Clone)]
pub struct Options {
    pub verbosity_level: LevelFilter,
    pub resource_db_path: PathBuf,
}

pub fn options() -> OptionParser<Options> {
    let verbosity_level = verbose_by_slice(
        3,
        [
            LevelFilter::OFF,
            LevelFilter::ERROR,
            LevelFilter::WARN,
            LevelFilter::INFO,
            LevelFilter::DEBUG,
            LevelFilter::TRACE,
        ],
    );

    let resource_db_path = long("db")
        .help("Selects the resources.db file to modify or create")
        .argument("FILE")
        .complete_shell(bpaf::ShellComp::File { mask: Some("*.db") });

    construct!(Options {
        verbosity_level,
        resource_db_path
    })
    .to_options()
}

#[cfg(test)]
mod tests {
    use super::options;

    #[test]
    fn check_bpaf_invariants() {
        options().check_invariants(true);
    }
}
