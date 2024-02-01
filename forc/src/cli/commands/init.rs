use crate::ops::forc_init;
use clap::Parser;
use forc_util::ForcResult;

forc_util::cli_examples! {
    [Initialize a new Forc project => forc "init --path /tmp/path-to-contract"]
    [Initialize a new Forc project as workspace => forc "init --path /tmp/path-to-contract --workspace"]
    setup {
        // make sure that /tmp/path-to-contract exists and it is empty
        let _ = std::fs::remove_dir_all("/tmp/path-to-contract");
        std::fs::create_dir("/tmp/path-to-contract").unwrap();
    }
}

/// Create a new Forc project in an existing directory.
#[derive(Debug, Parser)]
#[clap(bin_name = "forc init", version, after_help = help())]
pub struct Command {
    /// The directory in which the forc project will be initialized.
    #[clap(long)]
    pub path: Option<String>,
    /// The default program type, excluding all flags or adding this flag creates a basic contract program.
    #[clap(long)]
    pub contract: bool,
    /// Create a package with a script target (src/main.sw).
    #[clap(long)]
    pub script: bool,
    /// Create a package with a predicate target (src/predicate.rs).
    #[clap(long)]
    pub predicate: bool,
    /// Create a package with a library target (src/lib.sw).
    #[clap(long)]
    pub library: bool,
    /// Adding this flag creates an empty workspace.
    #[clap(long)]
    pub workspace: bool,
    /// Set the package name. Defaults to the directory name
    #[clap(long)]
    pub name: Option<String>,
}

pub(crate) fn exec(command: Command) -> ForcResult<()> {
    forc_init::init(command)?;
    Ok(())
}
