use std::io;

use clap::{Command, Parser, Subcommand};
use clap_complete::{Generator, Shell, generate};

#[derive(Parser, Debug)]
#[clap(name = "git-ignore", about, version, author)]
#[clap(args_conflicts_with_subcommands = true)]
/// Quickly and easily add templates to .gitignore
pub struct Cli {
    /// List <templates> or all available templates (uses gitignore.io cache).
    #[arg(short, long)]
    pub list: bool,
    /// Update templates by fetching them from gitignore.io
    #[arg(short = 'u', long)]
    pub update: bool,
    /// Autodetect templates based on the existing files (uses gitignore.io cache).
    #[arg(short, long)]
    pub auto: bool,
    /// Write to `.gitignore` file instead of stdout.
    /// For direct template fetching (e.g., `gi rust`), this appends to .gitignore.
    /// For gitignore.io cache operations, behavior depends on other flags.
    #[arg(short, long)]
    pub write: bool,
    /// Forcefully overwrite existing `.gitignore` file when used with gitignore.io cache operations.
    /// Not used by direct GitHub template fetching mode (which always appends if -w is active).
    #[arg(short, long, requires = "write")]
    pub force: bool,
    /// Verbose output.
    #[arg(short = 'v', long)]
    pub verbose: bool,
    /// Debug output.
    #[arg(long)]
    pub debug: bool,
    /// Configuration management
    #[command(subcommand)]
    pub cmd: Option<Cmds>,
    /// Names of templates to show/search for
    pub templates: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Cmds {
    #[command(subcommand, visible_alias = "aliases")]
    Alias(AliasCmd),
    #[command(subcommand, visible_alias = "templates")]
    Template(TemplateCmd),
    /// Initialize user configuration
    Init {
        /// Forcefully create config, possibly overwrite existing
        #[clap(long)]
        force: bool,
    },
    /// Generate shell completion
    Completion {
        /// Shell to generate completion for
        #[clap(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
/// Manage user defined aliases
///
/// Aliases are user defined mapping between a name and one or more other
/// templates and aliases and have preference over regular templates when
/// searching. So an `alias` called `node` that maps to `[node, deno]` will
/// write those to templates as a single one when running `git ignore node`.
pub enum AliasCmd {
    /// List available aliases
    #[command(visible_alias = "ls")]
    List,
    /// Add a new alias
    Add { name: String, aliases: Vec<String> },
    /// Remove an alias
    #[command(visible_alias = "rm")]
    Remove { name: String },
}

#[derive(Subcommand, Debug)]
/// Manage user defined templates
///
/// A template is a user defined ignore for applications or software that does
/// not exist in the existing database. These have the highest preference when
/// searching.
pub enum TemplateCmd {
    /// List available templates
    #[command(visible_alias = "ls")]
    List,
    /// Add a new template
    ///
    /// You'll need to edit the file created to finish creating a template
    Add { name: String },
    /// Remove a template
    #[command(visible_alias = "rm")]
    Remove { name: String },
}

pub fn print_completion<G: Generator>(generator: G, app: &mut Command) {
    generate(
        generator,
        app,
        app.get_name().to_string(),
        &mut io::stdout(),
    );
}
