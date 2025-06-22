use std::{
    collections::HashSet,
    env::current_dir,
    fs::{DirEntry, File, OpenOptions, read_dir, read_to_string},
    io::{self, Write as IoWrite}, // Renamed to avoid conflict
    path::Path,
    sync::LazyLock,
};

use anyhow::Result;
use colored::Colorize;
use etcetera::{AppStrategyArgs, choose_app_strategy};

use crate::{
    data::{CACHE_DIR, CACHE_FILE},
    detector::Detectors,
};

const GITHUB_GITIGNORE_BASE_URL: &str = "https://raw.githubusercontent.com/github/gitignore/main/";
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
    server: String,
    detectors: Detectors,
}

impl Core {
    /// Creates a new instance of the `git-ignore` program. Thanks to
    /// `directories` we support crossplatform caching of our results, the cache
    /// directories works on macOS, Linux and Windows. See the documentation for
    /// their locations.
    pub fn new() -> Self {
        Core {
            server: "https://www.gitignore.io/api/list?format=json".into(),
            detectors: Detectors::default(),
        }
    }

    /// Both updates and initializes `git-ignore`. Creates the cache directory
    /// if it doesn't exist and then downloads the templates from
    /// [gitignore.io](https://www.gitignore.io), saving them in the cache
    /// directory.
    pub fn update(&self) -> Result<()> {
        create_cache()?;
        self.fetch_gitignore()?;

        eprintln!("{}: Update successful", "Info".bold().green());
        Ok(())
    }

    /// Creates a formatted string of all the configured templates
    pub fn autodetect_templates(&self) -> Result<Vec<String>> {
        let entries: Vec<DirEntry> = read_dir(current_dir()?)?.map(Result::unwrap).collect();
        Ok(self.detectors.detects(entries.as_slice()))
    }

    /// Fetches all the templates from [gitignore.io](http://gitignore.io/),
    /// and writes the contents to the cache for easy future retrieval.
    fn fetch_gitignore(&self) -> Result<()> {
        let res = attohttpc::get(&self.server).send()?;

        let mut file = File::create(CACHE_FILE.as_path())?;
        file.write_all(&res.bytes()?)?;

        Ok(())
    }
}

pub fn cache_exists() -> bool {
    CACHE_DIR.exists() || CACHE_FILE.exists()
}

fn create_cache() -> std::io::Result<()> {
    if !cache_exists() {
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
