use std::{
    collections::HashSet,
    env::current_dir,
    fs::{DirEntry, File, OpenOptions, read_dir, read_to_string},
    io::{self, Write as IoWrite}, // Renamed to avoid conflict
    path::Path,
    process::Command, // Added for running git commands
    sync::LazyLock,
};

use anyhow::{Context, Result}; // Added Context
use colored::Colorize;
use etcetera::{AppStrategyArgs, choose_app_strategy};

use crate::{
    data::{CACHE_DIR, GIT_REPO_CACHE_DIR}, // Use GIT_REPO_CACHE_DIR, remove CACHE_FILE
    detector::Detectors,
};

const GITHUB_GITIGNORE_BASE_URL: &str = "https://raw.githubusercontent.com/github/gitignore/main/";
const GITHUB_GITIGNORE_REPO_URL: &str = "https://github.com/github/gitignore.git";
const GITIGNORE_FILE_NAME: &str = ".gitignore";

#[cfg(target_os = "windows")]
pub static PROJECT_DIRS: LazyLock<etcetera::app_strategy::Windows> = LazyLock::new(|| {
    choose_app_strategy(AppStrategyArgs {
        top_level_domain: "com".to_string(),
        author: "Sondre Aasemoen".to_string(),
        app_name: "git-ignore".to_string(),
    })
    .expect("Could not find project directory.")
});

#[cfg(not(target_os = "windows"))]
pub static PROJECT_DIRS: LazyLock<etcetera::app_strategy::Xdg> = LazyLock::new(|| {
    choose_app_strategy(AppStrategyArgs {
        top_level_domain: "com".to_string(),
        author: "Sondre Aasemoen".to_string(),
        app_name: "git-ignore".to_string(),
    })
    .expect("Could not find project directory.")
});

#[derive(Debug)]
pub struct Core {
    // server field removed
    detectors: Detectors,
}

impl Core {
    /// Creates a new instance of the `git-ignore` program.
    /// Caching uses a local clone of the github/gitignore repository.
    pub fn new() -> Self {
        Core {
            // server initialization removed
            detectors: Detectors::default(),
        }
    }

    /// Updates the local cache of the github/gitignore repository.
    /// Clones the repository if it doesn't exist, or pulls the latest changes if it does.
    /// Requires `git` to be installed and in PATH.
    pub fn update(&self) -> Result<()> {
        // Ensure the base cache directory exists. GIT_REPO_CACHE_DIR will be created by git clone.
        if !CACHE_DIR.exists() {
            std::fs::create_dir_all(CACHE_DIR.as_path())
                .with_context(|| format!("Failed to create cache directory at {:?}", CACHE_DIR.as_path()))?;
            eprintln!("{}: Created cache directory at {}", "Info".bold().green(), CACHE_DIR.display());
        }

        if GIT_REPO_CACHE_DIR.exists() {
            eprintln!(
                "{}: Attempting to update existing local gitignore repository cache at {}...",
                "Info".bold().green(),
                GIT_REPO_CACHE_DIR.display()
            );
            let output = Command::new("git")
                .arg("-C")
                .arg(GIT_REPO_CACHE_DIR.as_path())
                .arg("pull")
                .output()
                .with_context(|| format!("Failed to execute 'git pull' in {:?}", GIT_REPO_CACHE_DIR.as_path()))?;

            if output.status.success() {
                eprintln!(
                    "{}: Successfully updated local gitignore repository.",
                    "Info".bold().green()
                );
                if !output.stdout.is_empty() {
                    eprintln!("Git pull output:\n{}", String::from_utf8_lossy(&output.stdout));
                }
            } else {
                eprintln!(
                    "{}: Failed to update local gitignore repository. 'git pull' exited with status: {}",
                    "Error".bold().red(),
                    output.status
                );
                if !output.stderr.is_empty() {
                    eprintln!("Git pull error:\n{}", String::from_utf8_lossy(&output.stderr));
                }
                // Optionally, could suggest deleting the cache dir and retrying.
            }
        } else {
            eprintln!(
                "{}: Local gitignore repository cache not found. Cloning from {} to {}...",
                "Info".bold().green(),
                GITHUB_GITIGNORE_REPO_URL,
                GIT_REPO_CACHE_DIR.display()
            );
            let output = Command::new("git")
                .arg("clone")
                .arg(GITHUB_GITIGNORE_REPO_URL)
                .arg(GIT_REPO_CACHE_DIR.as_path())
                .output()
                .with_context(|| format!("Failed to execute 'git clone {}'", GITHUB_GITIGNORE_REPO_URL))?;

            if output.status.success() {
                eprintln!(
                    "{}: Successfully cloned gitignore repository.",
                    "Info".bold().green()
                );
            } else {
                eprintln!(
                    "{}: Failed to clone gitignore repository. 'git clone' exited with status: {}",
                    "Error".bold().red(),
                    output.status
                );
                if !output.stderr.is_empty() {
                    eprintln!("Git clone error:\n{}", String::from_utf8_lossy(&output.stderr));
                }
                // Optionally, could suggest checking git installation or network.
            }
        }
        Ok(())
    }

    /// Autodetects templates based on files in the current directory.
    /// This uses the locally cached github/gitignore repository.
    pub fn autodetect_templates(&self) -> Result<Vec<String>> {
        let entries: Vec<DirEntry> = read_dir(current_dir()?)?.map(Result::unwrap).collect();
        Ok(self.detectors.detects(entries.as_slice()))
    }

    // fetch_gitignore method removed as it's no longer used.
}

pub fn cache_exists() -> bool {
    // Now checks for the existence of the git repository cache directory
    GIT_REPO_CACHE_DIR.exists() && GIT_REPO_CACHE_DIR.is_dir()
}

fn create_cache() -> std::io::Result<()> {
    // This function now only ensures the top-level cache directory exists.
    // The specific git repo cache directory (GIT_REPO_CACHE_DIR)
    // will be created by `git clone` if it doesn't exist.
    if !CACHE_DIR.exists() {
        std::fs::create_dir_all(CACHE_DIR.as_path())?;
    }
    Ok(())
}

// Helper function to apply capitalization similar to the Zsh script's logic.
fn capitalize_template_spec(spec: &str, debug: bool) -> String {
    let parts: Vec<String> = spec
        .split('/')
        .map(|part| {
            if part.chars().any(|c| c.is_ascii_uppercase()) {
                part.to_string()
            } else {
                let mut capitalized_part = String::new();
                let mut capitalize_next = true;
                for c in part.chars() {
                    if capitalize_next && c.is_ascii_lowercase() {
                        capitalized_part.push(c.to_ascii_uppercase());
                        capitalize_next = false;
                    } else {
                        capitalized_part.push(c);
                    }
                    // A non-alphanumeric char sets up the next char for capitalization
                    if !c.is_alphanumeric() {
                        capitalize_next = true;
                    } else if capitalize_next {
                        // if it was an alphanumeric char that got capitalized, or was already uppercase
                        capitalize_next = false;
                    }
                }
                capitalized_part
            }
        })
        .collect();
    let result = parts.join("/");
    if debug {
        eprintln!(
            "DEBUG: Capitalization: original='{}', corrected='{}'",
            spec, result
        );
    }
    result
}

/// Fetches templates directly from github/gitignore and appends them to the local .gitignore file or prints to stdout.
pub fn fetch_and_append_github_templates(
    template_specs: &[String],
    verbose: bool,
    debug: bool,
    write_to_file_flag: bool,
    // force_write is not used by this function as it always appends if write_to_file_flag is true.
) -> Result<()> {
    if debug {
        eprintln!("DEBUG: fetch_and_append_github_templates ENTERED");
    }

    if template_specs.is_empty() {
        // This should ideally be caught by clap if templates are required.
        eprintln!("{}", "Error: No gitignore template specified.".red());
        // Consider printing usage instructions or returning an error that main can handle.
        return Ok(());
    }

    let gitignore_path = Path::new(GITIGNORE_FILE_NAME);
    let mut existing_lines = HashSet::new();
    // Collects all unique new lines from all templates for this session, to be written/printed once.
    let mut session_lines_to_add = Vec::new();

    if write_to_file_flag {
        if !gitignore_path.exists() {
            File::create(gitignore_path)?;
            if verbose {
                eprintln!("VERBOSE: Created {}.", GITIGNORE_FILE_NAME.cyan());
            }
        }

        match read_to_string(gitignore_path) {
            Ok(content) => {
                for line in content.lines() {
                    existing_lines.insert(line.trim_end().to_string());
                }
                if debug {
                    eprintln!(
                        "DEBUG: Loaded {} lines from existing .gitignore.",
                        existing_lines.len()
                    );
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if debug {
                    eprintln!("DEBUG: .gitignore not found, will be created if lines are added.");
                }
            }
            Err(e) => {
                return Err(anyhow::Error::new(e)
                    .context(format!("Failed to read {}", GITIGNORE_FILE_NAME)));
            }
        }
    }

    let mut overall_new_lines_count_for_session = 0;
    let mut succeeded_templates_list = String::new();
    let mut failed_templates_list = String::new();

    for template_spec_original in template_specs {
        let template_spec_for_url = capitalize_template_spec(template_spec_original, debug);

        if verbose {
            eprintln!(
                "\nVERBOSE: Processing template: '{}' (attempting as '{}')...",
                template_spec_original.cyan(),
                template_spec_for_url.cyan()
            );
        }

        let template_file_path_in_repo = format!("{}.gitignore", template_spec_for_url);
        let fetch_url = format!(
            "{}{}",
            GITHUB_GITIGNORE_BASE_URL, template_file_path_in_repo
        );

        if verbose {
            eprintln!("VERBOSE: Fetching from: {}", fetch_url.yellow());
        }

        let response = attohttpc::get(&fetch_url).send();
        // let mut current_template_had_content = false; // This variable was unused

        match response {
            Ok(res) => {
                if res.is_success() {
                    let body = res.text()?;
                    // current_template_had_content = !body.is_empty(); // Assignment removed
                    succeeded_templates_list.push_str(&format!("{} ", template_spec_original));

                    if body.is_empty() && verbose {
                        eprintln!(
                            "VERBOSE: Note: Template '{}' (fetched as '{}') is empty.",
                            template_spec_original.cyan(),
                            template_spec_for_url.cyan()
                        );
                    }

                    let mut current_template_new_lines_added_to_session = 0;
                    let mut current_template_existed_lines = 0;

                    for line_raw in body.lines() {
                        let line = line_raw.trim_end();

                        if line.is_empty() {
                            if verbose {
                                eprintln!("VERBOSE: Skipping empty line from template.");
                            }
                            continue;
                        }

                        if verbose {
                            eprintln!("VERBOSE: Checking line: '{}'", line);
                        }

                        if existing_lines.contains(line) {
                            if verbose {
                                eprintln!("VERBOSE: Line already exists: '{}'", line.italic());
                            }
                            current_template_existed_lines += 1;
                        } else {
                            if verbose {
                                eprintln!(
                                    "VERBOSE: New line, collecting for session: '{}'",
                                    line.green()
                                );
                            }
                            session_lines_to_add.push(line.to_string());
                            existing_lines.insert(line.to_string()); // Mark as existing for subsequent templates in this run
                            current_template_new_lines_added_to_session += 1;
                        }
                    }
                    overall_new_lines_count_for_session +=
                        current_template_new_lines_added_to_session;

                    if write_to_file_flag && current_template_new_lines_added_to_session > 0 {
                        // Message per template if writing to file and new lines were found for *this* template
                        println!(
                            "Collected {} new line(s) from '{}' for current session.",
                            current_template_new_lines_added_to_session,
                            template_spec_original.cyan()
                        );
                    }

                    if current_template_new_lines_added_to_session == 0
                        && current_template_existed_lines > 0
                        // && current_template_had_content // Condition removed as variable is removed
                    {
                        // If the template had content (checked by body.is_empty() earlier)
                        // and no new lines were added, but some existed, this message is appropriate.
                        // The check for `body.is_empty()` at the beginning of the success block
                        // already handles the case for truly empty templates.
                        if verbose || write_to_file_flag {
                            // Show this if writing or verbose
                            println!(
                                "All patterns from '{}' (fetched as '{}') already existed or were duplicates (template was not empty).",
                                template_spec_original.cyan(),
                                template_spec_for_url.cyan()
                            );
                        }
                    }
                } else {
                    eprintln!(
                        "{}: Failed to fetch template '{}' (tried as '{}') - HTTP Status: {}",
                        "Error".red().bold(),
                        template_spec_original.cyan(),
                        template_spec_for_url.cyan(),
                        res.status().as_str().yellow()
                    );
                    failed_templates_list.push_str(&format!("{} ", template_spec_original));
                }
            }
            Err(e) => {
                eprintln!(
                    "{}: Failed to fetch template '{}' (tried as '{}') - Error: {}",
                    "Error".red().bold(),
                    template_spec_original.cyan(),
                    template_spec_for_url.cyan(),
                    e.to_string().yellow()
                );
                failed_templates_list.push_str(&format!("{} ", template_spec_original));
            }
        }
    }

    if write_to_file_flag {
        if !session_lines_to_add.is_empty() {
            // Check if there are any lines collected from *any* template
            let mut file = OpenOptions::new().append(true).open(gitignore_path)?;

            // Check if .gitignore needs a newline before appending
            let current_content_for_newline_check = read_to_string(gitignore_path)?;
            if !current_content_for_newline_check.is_empty()
                && !current_content_for_newline_check.ends_with('\n')
            {
                if verbose {
                    eprintln!(
                        "VERBOSE: Adding newline to end of {} before appending.",
                        GITIGNORE_FILE_NAME.cyan()
                    );
                }
                writeln!(file)?;
            }

            for line in &session_lines_to_add {
                writeln!(file, "{}", line)?;
            }

            println!(
                "Total {} new line(s) appended to {}.",
                overall_new_lines_count_for_session,
                GITIGNORE_FILE_NAME.cyan()
            );
        } else if !succeeded_templates_list.trim().is_empty() {
            println!(
                "No new lines were added to {} from the processed templates.",
                GITIGNORE_FILE_NAME.cyan()
            );
        }
    } else {
        // Write to stdout
        if !session_lines_to_add.is_empty() {
            for line in &session_lines_to_add {
                println!("{}", line);
            }
        } else if !succeeded_templates_list.trim().is_empty() && verbose {
            eprintln!("No new lines to output to stdout from the processed templates.");
        }
    }

    if verbose {
        eprint!("\nVERBOSE: ");
    }
    if !succeeded_templates_list.trim().is_empty() {
        println!(
            "Successfully processed template(s): {}",
            succeeded_templates_list.trim().green()
        );
    }
    if !failed_templates_list.trim().is_empty() {
        eprintln!(
            "{}: {}",
            "Failed to fetch or process template(s)".red(),
            failed_templates_list.trim().yellow()
        );
    }

    if debug {
        eprintln!("DEBUG: fetch_and_append_github_templates normal exit");
    }
    Ok(())
}
