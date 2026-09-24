use std::collections::BTreeMap;
use std::fmt;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    #[value(alias = "pwsh")]
    Powershell,
}

impl Shell {
    pub const ALL: [Shell; 4] = [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Powershell];

    pub fn name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Powershell => "powershell",
        }
    }

    pub fn init_extension(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Powershell => "ps1",
        }
    }

    /// Executables that mean this shell is installed.
    pub fn binaries(self) -> &'static [&'static str] {
        match self {
            Shell::Bash => &["bash"],
            Shell::Zsh => &["zsh"],
            Shell::Fish => &["fish"],
            Shell::Powershell => &["pwsh", "powershell"],
        }
    }
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    #[value(alias = "mac", alias = "darwin")]
    Macos,
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Os::Macos => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Alias {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// Empty means every shell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shells: Vec<Shell>,
    /// Empty means every OS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<Os>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Ask "run this?" every time the alias is used.
    #[serde(default, skip_serializing_if = "is_false")]
    pub confirm: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_at: String,
}

impl Alias {
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            description: None,
            enabled: true,
            shells: Vec::new(),
            os: Vec::new(),
            locked: false,
            confirm: false,
            created_at: now(),
        }
    }

    pub fn applies_to(&self, shell: Shell, os: Os) -> bool {
        self.enabled
            && (self.shells.is_empty() || self.shells.contains(&shell))
            && (self.os.is_empty() || self.os.contains(&os))
    }

    /// Short labels for anything unusual about this alias, used by `list` and `show`.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if !self.enabled {
            notes.push("disabled".to_string());
        }
        if self.locked {
            notes.push("locked".to_string());
        }
        if self.confirm {
            notes.push("asks first".to_string());
        }
        if !self.shells.is_empty() {
            notes.push(join(&self.shells, "/") + " only");
        }
        if !self.os.is_empty() {
            notes.push(join(&self.os, "/") + " only");
        }
        notes
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AliasFile {
    #[serde(default = "format_version")]
    pub version: u32,
    #[serde(default)]
    pub aliases: BTreeMap<String, Alias>,
}

impl Default for AliasFile {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            aliases: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trashed {
    #[serde(flatten)]
    pub alias: Alias,
    pub deleted_at: String,
}

/// Removed aliases, oldest first for each name. Removing the same name twice
/// keeps both versions; `aka restore` brings back the newest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrashFile {
    #[serde(default)]
    pub trash: BTreeMap<String, Vec<Trashed>>,
}

impl TrashFile {
    pub fn push(&mut self, name: &str, alias: Alias) {
        self.trash
            .entry(name.to_string())
            .or_default()
            .push(Trashed {
                alias,
                deleted_at: now(),
            });
    }

    /// Takes out the most recently removed version of `name`.
    pub fn pop(&mut self, name: &str) -> Option<Trashed> {
        let versions = self.trash.get_mut(name)?;
        let newest = versions.pop();
        if versions.is_empty() {
            self.trash.remove(name);
        }
        newest
    }

    pub fn contains(&self, name: &str) -> bool {
        self.trash.contains_key(name)
    }

    pub fn len(&self) -> usize {
        self.trash.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.trash.is_empty()
    }

    /// Every trashed version, grouped by name, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Trashed)> {
        self.trash
            .iter()
            .flat_map(|(name, versions)| versions.iter().map(move |t| (name, t)))
    }
}

/// Everything aka persists, loaded and saved together so undo can restore it as one unit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub aliases: AliasFile,
    pub trash: TrashFile,
}

impl State {
    pub fn get(&self, name: &str) -> Option<&Alias> {
        self.aliases.aliases.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Alias> {
        self.aliases.aliases.get_mut(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.aliases.aliases.contains_key(name)
    }

    pub fn insert(&mut self, name: impl Into<String>, alias: Alias) {
        self.aliases.aliases.insert(name.into(), alias);
    }

    pub fn remove(&mut self, name: &str) -> Option<Alias> {
        self.aliases.aliases.remove(name)
    }
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn join<T: fmt::Display>(items: &[T], sep: &str) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(sep)
}

fn default_true() -> bool {
    true
}

fn format_version() -> u32 {
    FORMAT_VERSION
}

fn is_true(v: &bool) -> bool {
    *v
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut state = AliasFile::default();
        let mut gs = Alias::new("git status");
        gs.description = Some("quick status".into());
        gs.shells = vec![Shell::Zsh, Shell::Bash];
        gs.locked = true;
        state.aliases.insert("gs".into(), gs);
        state.aliases.insert("ll".into(), Alias::new("ls -la"));

        let text = toml::to_string_pretty(&state).unwrap();
        let back: AliasFile = toml::from_str(&text).unwrap();
        assert_eq!(state, back);
        assert!(
            !text.contains("enabled"),
            "defaults should stay out of the file"
        );
    }

    #[test]
    fn trash_keeps_every_version() {
        let mut trash = TrashFile::default();
        trash.push("gs", Alias::new("git status"));
        trash.push("gs", Alias::new("git status -sb"));
        assert_eq!(trash.len(), 2);

        let text = toml::to_string_pretty(&trash).unwrap();
        let mut back: TrashFile = toml::from_str(&text).unwrap();
        assert_eq!(back, trash);

        assert_eq!(back.pop("gs").unwrap().alias.command, "git status -sb");
        assert_eq!(back.pop("gs").unwrap().alias.command, "git status");
        assert!(back.pop("gs").is_none());
        assert!(back.is_empty());
    }

    #[test]
    fn minimal_entries_get_defaults() {
        let file: AliasFile = toml::from_str("[aliases.gs]\ncommand = \"git status\"\n").unwrap();
        let gs = &file.aliases["gs"];
        assert!(gs.enabled);
        assert!(gs.applies_to(Shell::Fish, Os::Linux));
        assert_eq!(file.version, FORMAT_VERSION);
    }

    #[test]
    fn filters_by_shell_and_os() {
        let mut a = Alias::new("open .");
        a.os = vec![Os::Macos];
        a.shells = vec![Shell::Zsh];
        assert!(a.applies_to(Shell::Zsh, Os::Macos));
        assert!(!a.applies_to(Shell::Bash, Os::Macos));
        assert!(!a.applies_to(Shell::Zsh, Os::Linux));
        a.enabled = false;
        assert!(!a.applies_to(Shell::Zsh, Os::Macos));
    }
}
