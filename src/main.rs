#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod cli;
mod data;
mod detector;
mod ignore;
mod user_data;

use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{self, Write},
};

use anyhow::Result;
use clap::{CommandFactory, Parser};
use cli::{AliasCmd, Cli, Cmds, TemplateCmd, print_completion};
use colored::Colorize;
use ignore::Core;
use user_data::UserData;

use crate::{
    data::{IgnoreData, get_templates, list},
    ignore::cache_exists,
};

fn main() -> Result<()> {
    let opt = Cli::parse();

    if opt.debug {
        eprintln!("DEBUG: Parsed CLI options: {:?}", opt);
    }

    // Handle subcommands first
    if let Some(cmd) = opt.cmd {
        if opt.debug {
            eprintln!("DEBUG: Handling subcommand: {:?}", cmd);
        }
        // Initialize UserData and IgnoreData only if needed by a subcommand
        match cmd {
            Cmds::Init { force } => return UserData::create(force),
            Cmds::Alias(alias_cmd) => {
                let mut user_data = UserData::new()?;
                let ignore_data = IgnoreData::new(&user_data)?;
                return match alias_cmd {
                    AliasCmd::List => {
                        ignore_data.list_aliases();
                        Ok(())
                    }
                    AliasCmd::Add { name, aliases } => user_data.add_alias(name, aliases),
                    AliasCmd::Remove { name } => user_data.remove_alias(&name),
                };
            }
            Cmds::Template(template_cmd) => {
                let mut user_data = UserData::new()?;
                let ignore_data = IgnoreData::new(&user_data)?;
                return match template_cmd {
                    TemplateCmd::List => {
                        ignore_data.list_templates();
                        Ok(())
                    }
                    TemplateCmd::Add { name } => user_data.add_template(name),
                    TemplateCmd::Remove { name } => user_data.remove_template(&name),
                };
            }
            Cmds::Completion { shell } => {
                let mut app_cmd = Cli::command();
                print_completion(shell, &mut app_cmd);
                return Ok(());
            }
        }
    }

    // If no subcommand, and templates are provided directly, and it's not a list/update/auto for gitignore.io
    if !opt.templates.is_empty() && !opt.list && !opt.update && !opt.auto {
        if opt.debug {
            eprintln!("DEBUG: Entering direct GitHub template fetch mode.");
        }
        return ignore::fetch_and_append_github_templates(
            &opt.templates,
            opt.verbose,
            opt.debug,
            opt.write,
        );
    }

    // --- Existing logic for gitignore.io cache, list, auto, etc. ---
    if opt.debug {
        eprintln!("DEBUG: Entering gitignore.io cache logic mode.");
    }

    let app = Core::new();
    let user_data = UserData::new()?; // Removed mut, as it's not mutated in this path
    let ignore_data = IgnoreData::new(&user_data)?;

    if opt.update {
        if opt.verbose {
            eprintln!("VERBOSE: Updating gitignore.io cache...");
        }
        app.update()?; // This prints "Info: Update successful"
        if opt.templates.is_empty() && !opt.auto && !opt.list {
            if opt.debug {
                eprintln!("DEBUG: Update complete, no further templates to process. Exiting.");
            }
            return Ok(());
        }
    } else if cache_exists() {
        if opt.verbose || (!opt.list && !opt.templates.is_empty()) {
            eprintln!(
                "{}: You are using cached results from gitignore.io, pass '-u' to update the cache\n",
                "Info".bold().green(),
            );
        }
    } else if !opt.list {
        eprintln!(
            "{}: Cache directory or gitignore.io ignore file not found, attempting update.",
            "Warning".bold().red(),
        );
        app.update()?;
    }

    let mut all_templates_for_cache: HashSet<String> = opt.templates.into_iter().collect();
    if opt.auto {
        if opt.verbose {
            eprintln!("VERBOSE: Autodetecting templates for gitignore.io cache...");
        }
        for template in app.autodetect_templates()? {
            if opt.verbose {
                eprintln!("VERBOSE: Autodetected (for cache): {}", template.cyan());
            }
            all_templates_for_cache.insert(template);
        }
    }

    let templates_for_cache: Vec<String> = all_templates_for_cache.iter().cloned().collect();

    if opt.update && templates_for_cache.is_empty() && !opt.list {
        if opt.debug {
            eprintln!(
                "DEBUG: Update was run, but no templates specified for further processing via cache. Exiting."
            );
        }
        return Ok(());
    }

    let output_str = if opt.list {
        if opt.verbose {
            eprintln!(
                "VERBOSE: Listing templates from gitignore.io cache for: {:?}",
                templates_for_cache
            );
        }
        list(&ignore_data, templates_for_cache.as_slice())
    } else if templates_for_cache.is_empty() {
        if opt.debug {
            eprintln!(
                "DEBUG: No templates specified for gitignore.io cache processing, rendering help."
            );
        }
        let mut app_cmd = Cli::command();
        app_cmd.render_help().to_string()
    } else {
        if opt.verbose {
            eprintln!(
                "VERBOSE: Getting templates from gitignore.io cache for: {:?}",
                templates_for_cache
            );
        }
        get_templates(&ignore_data, templates_for_cache.as_slice())
    };

    if output_str.is_empty() && templates_for_cache.is_empty() && !opt.list {
        // Help was rendered into output_str.
        if opt.debug {
            eprintln!("DEBUG: Output string is empty (help was rendered).");
        }
    } else if output_str.is_empty() && !templates_for_cache.is_empty() {
        eprintln!(
            "{}: No templates found in gitignore.io cache for: {}",
            "Warning".yellow(),
            templates_for_cache.join(", ")
        );
        return Ok(());
    }

    if opt.write {
        if opt.debug {
            eprintln!("DEBUG: Write flag is set for gitignore.io cache output.");
        }
        let file_path = std::env::current_dir()?.join(".gitignore");
        if !file_path.exists() {
            if opt.verbose {
                eprintln!(
                    "VERBOSE: no '.gitignore' file found, creating with content from gitignore.io...",
                );
            }
            let mut file = File::create(&file_path)?;
            file.write_all(output_str.as_bytes())?;
            println!(
                "Created {} with content from gitignore.io for: {}",
                ".gitignore".cyan(),
                templates_for_cache.join(", ").green()
            );
        } else if opt.force {
            if opt.verbose {
                eprintln!(
                    "VERBOSE: appending results from gitignore.io to '.gitignore' (force active)...",
                );
            }
            let mut file = OpenOptions::new().append(true).open(&file_path)?;
            let current_content = std::fs::read_to_string(&file_path)?; // Use std::fs for simplicity here
            if !current_content.is_empty() && !current_content.ends_with('\n') {
                writeln!(file)?;
            }
            file.write_all(output_str.as_bytes())?;
            println!(
                "Appended content from gitignore.io to {} for: {}",
                ".gitignore".cyan(),
                templates_for_cache.join(", ").green()
            );
        } else {
            eprintln!(
                "{}: '.gitignore' already exists. Use '-f' to append results from gitignore.io, or handle manually.",
                "Warning".bold().red()
            );
        }
    } else {
        if opt.debug {
            eprintln!("DEBUG: Writing gitignore.io cache output to stdout.");
        }
        let stdout_handle = io::stdout();
        let mut locked_stdout = stdout_handle.lock();
        locked_stdout.write_all(output_str.as_bytes())?;
    }

    Ok(())
}
