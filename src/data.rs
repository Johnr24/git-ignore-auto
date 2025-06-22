use std::{
    cmp::Ordering,
    // collections::HashMap, // No longer directly used here
    fmt::{Display, Write, write},
    fs::{read_to_string, read_dir},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, Result};
use colored::Colorize;
use etcetera::AppStrategy;
// serde is not directly used in this file anymore for Language struct
// use serde::{Deserialize, Serialize}; 

use crate::{ignore::PROJECT_DIRS, user_data::UserData};

pub static CACHE_DIR: LazyLock<PathBuf> = LazyLock::new(|| PROJECT_DIRS.cache_dir());
pub static GIT_REPO_CACHE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| CACHE_DIR.join("github_gitignore_repo"));
// CACHE_FILE is no longer needed as we're not using ignore.json from gitignore.io

// Language struct is no longer needed as we parse files directly
// #[derive(Deserialize, Serialize, Debug)]
// pub struct Language {
// key: String,
// name: String,
// #[serde(rename = "fileName")]
// file_name: String,
// pub contents: String,
// }

#[derive(Debug)]
pub struct IgnoreData {
    pub data: Vec<Type>,
}

fn read_templates_from_dir(dir_path: &Path, base_key_prefix: Option<&str>) -> Result<Vec<Type>> {
    let mut templates = Vec::new();
    if !dir_path.exists() || !dir_path.is_dir() {
        // It's okay if a subdirectory like Global doesn't exist or if the main repo isn't cloned yet.
        // This function is for reading, not for ensuring existence.
        return Ok(templates);
    }

    for entry in read_dir(dir_path).with_context(|| format!("Failed to read directory: {:?}", dir_path))? {
        let entry = entry.with_context(|| format!("Failed to read directory entry in {:?}", dir_path))?;
        let path = entry.path();
        if path.is_file() {
            if let Some(extension) = path.extension() {
                if extension == "gitignore" {
                    if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let key = match base_key_prefix {
                            Some(prefix) => format!("{}/{}", prefix, file_stem),
                            None => file_stem.to_string(),
                        };
                        let content = read_to_string(&path)
                            .with_context(|| format!("Failed to read template file: {:?}", path))?;
                        templates.push(Type::Template { key, content });
                    }
                }
            }
        }
    }
    Ok(templates)
}

impl IgnoreData {
    pub fn new(user_data: &UserData) -> Result<Self> {
        let mut data: Vec<Type> = Vec::new();

        // Read templates from the root of the cloned gitignore repository
        data.extend(read_templates_from_dir(GIT_REPO_CACHE_DIR.as_path(), None)?);

        // Read templates from the Global/ subdirectory of the cloned gitignore repository
        let global_dir_path = GIT_REPO_CACHE_DIR.join("Global");
        data.extend(read_templates_from_dir(&global_dir_path, Some("Global"))?);
        
        // If data is empty at this point, it means the cache might not be populated.
        // The `Core::update` logic (which will handle git clone/pull) should run before this,
        // or this function should handle the "not yet cloned" case gracefully (which it does by returning empty vec).

        data.extend(
            user_data
                .aliases
                .clone()
                .into_iter()
                .map(|(k, v)| Type::Alias { key: k, aliases: v }),
        );

        let user_templates: Vec<_> = user_data
            .templates
            .clone()
            .into_iter()
            .map(|(name, path)| {
                let template = UserData::read_template(&path)?;
                Ok(Type::UserTemplate {
                    key: name,
                    content: template,
                })
            })
            .collect::<Result<_>>()?;
        data.extend(user_templates);

        data.sort_unstable();

        Ok(IgnoreData { data })
    }

    pub fn keys(&self) -> impl Iterator<Item = TypeName> {
        self.data.iter().map(TypeName::from)
    }

    pub fn list_aliases(&self) {
        let aliases = self
            .data
            .iter()
            .filter(|v| matches!(v, Type::Alias { .. }))
            .collect::<Vec<_>>();

        if aliases.is_empty() {
            return println!("{}", "No aliases defined".blue());
        }

        println!("{}", "Available aliases:".bold().green());
        for kind in aliases {
            println!(
                "{} => {:?}",
                TypeName::from(kind),
                self.get_alias(kind.key())
                    .expect("Found alias is missing, this is an internal error")
            );
        }
    }

    pub fn list_templates(&self) {
        let templates = self
            .data
            .iter()
            .filter(|v| matches!(v, Type::UserTemplate { .. }))
            .collect::<Vec<_>>();

        if templates.is_empty() {
            return println!("{}", "No templates defined".blue());
        }

        println!("{}", "Available templates:".bold().green());
        for kind in templates {
            println!(
                "{}:\n{}",
                TypeName::from(kind),
                self.get_user_template(kind.key())
                    .expect("Found template is missing, this is an internal error")
            );
        }
    }

    pub fn get_template(&self, name: &str) -> Option<String> {
        self.data
            .iter()
            .find(|k| matches!(k,Type::Template { key, .. } if key == name))
            .map(|v| match v {
                Type::Template { content, .. } => content.clone(),
                _ => unreachable!(),
            })
    }

    pub fn get_alias(&self, name: &str) -> Option<Vec<String>> {
        self.data
            .iter()
            .find(|k| matches!(k,Type::Alias { key, .. } if key == name))
            .map(|v| match v {
                Type::Alias { aliases, .. } => aliases.clone(),
                _ => unreachable!(),
            })
    }

    pub fn get_user_template(&self, name: &str) -> Option<String> {
        self.data
            .iter()
            .find(|k| matches!(k,Type::UserTemplate { key, .. } if key == name))
            .map(|v| match v {
                Type::UserTemplate { content, .. } => content.clone(),
                _ => unreachable!(),
            })
    }
}

#[derive(Debug, Clone)]
pub enum Type {
    Template { key: String, content: String },
    Alias { key: String, aliases: Vec<String> },
    UserTemplate { key: String, content: String },
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Template { key: k1, .. }, Type::Template { key: k2, .. })
            | (Type::Alias { key: k1, .. }, Type::Alias { key: k2, .. })
            | (Type::UserTemplate { key: k1, .. }, Type::UserTemplate { key: k2, .. }) => k1 == k2,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl PartialOrd for Type {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Type {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Type::UserTemplate { .. }, Type::UserTemplate { .. }) => Ordering::Equal,
            (Type::UserTemplate { .. }, _) => Ordering::Greater,
            (Type::Alias { .. }, Type::UserTemplate { .. }) => Ordering::Greater,
            (Type::Alias { .. }, Type::Alias { .. }) => Ordering::Equal,
            (Type::Alias { .. }, Type::Template { .. }) => Ordering::Less,
            (Type::Template { .. }, Type::Template { .. }) => Ordering::Equal,
            (Type::Template { .. }, _) => Ordering::Less,
        }
    }
}

impl Type {
    pub fn key(&self) -> &str {
        match self {
            Self::Template { key, .. }
            | Self::Alias { key, .. }
            | Self::UserTemplate { key, .. } => key,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TypeName {
    Template(String),
    Alias(String),
    UserTemplate(String),
}

impl From<&Type> for TypeName {
    fn from(value: &Type) -> Self {
        match value {
            Type::Template { key, .. } => TypeName::Template(key.clone()),
            Type::Alias { key, .. } => TypeName::Alias(key.clone()),
            Type::UserTemplate { key, .. } => TypeName::UserTemplate(key.clone()),
        }
    }
}

impl Display for TypeName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeName::Template(name) => write(f, format_args!("{}", name)),
            TypeName::Alias(name) => write(f, format_args!("{}", name.yellow().bold())),
            TypeName::UserTemplate(name) => write(f, format_args!("{}", name.blue().bold())),
        }
    }
}

impl PartialEq for TypeName {
    fn eq(&self, other: &Self) -> bool {
        self.inner() == other.inner()
    }
}

impl Eq for TypeName {}

impl PartialOrd for TypeName {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TypeName {
    fn cmp(&self, other: &Self) -> Ordering {
        self.inner().cmp(other.inner())
    }
}

impl Hash for TypeName {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.inner().hash(state);
    }
}

impl TypeName {
    fn inner(&self) -> &str {
        match self {
            TypeName::Template(name) | TypeName::Alias(name) | TypeName::UserTemplate(name) => name,
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        let inner = self.inner();
        inner.contains(name)
    }
}

pub fn list(data: &IgnoreData, names: &[String]) -> String {
    let templates = data.keys();

    let mut result = if names.is_empty() {
        templates.into_iter().collect::<Vec<_>>()
    } else {
        let mut result = Vec::new();

        for entry in templates {
            for name in names {
                if entry.contains(name) {
                    result.push(entry.clone());
                }
            }
        }
        result
    };

    result.sort_unstable();

    result.into_iter().fold(String::new(), |mut s, r| {
        writeln!(s, "  {r}").unwrap();
        s
    })
}

pub fn get_templates(data: &IgnoreData, names: &[String]) -> String {
    let mut result = String::new();

    for name in names {
        if let Some(val) = data.get_user_template(name) {
            result.push_str(&val);
        } else if let Some(val) = data.get_alias(name) {
            for alias in val {
                if let Some(val) = data.get_user_template(&alias) {
                    result.push_str(&val);
                } else if let Some(language) = data.get_template(&alias) {
                    result.push_str(&language);
                } else {
                    eprintln!("{}: No such alias", name.bold().yellow());
                }
            }
        } else if let Some(language) = data.get_template(name) {
            result.push_str(&language);
        }
    }

    if !result.is_empty() {
        // Prepend a header indicating the source of the combined templates.
        // The actual content comes from individual files in github/gitignore.
        let mut header = format!(
            "\n\n### Sourced from github/gitignore for: {} ###\n",
            names.join(", ")
        );
        header.push_str(&result);
        result = header;
    }

    result
}
