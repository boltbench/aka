use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::Shell;

/// Every file aka reads or writes lives under `root`.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
    pub home: PathBuf,
}

impl Paths {
    /// `AKA_HOME` wins, then `XDG_CONFIG_HOME/aka`, then the platform default
    /// (`%APPDATA%\aka` on Windows, `~/.config/aka` everywhere else).
    pub fn resolve() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .or_else(dirs::home_dir)
            .context("couldn't find your home directory")?;

        let root = if let Some(dir) = env_path("AKA_HOME") {
            dir
        } else if let Some(dir) = env_path("XDG_CONFIG_HOME") {
            dir.join("aka")
        } else if cfg!(windows) {
            dirs::config_dir()
                .unwrap_or_else(|| home.join("AppData").join("Roaming"))
                .join("aka")
        } else {
            home.join(".config").join("aka")
        };

        Ok(Self { root, home })
    }

    pub fn aliases_file(&self) -> PathBuf {
        self.root.join("aliases.toml")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn trash_file(&self) -> PathBuf {
        self.root.join("trash.toml")
    }

    pub fn history_file(&self) -> PathBuf {
        self.root.join("history.log")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    pub fn profile_backups_dir(&self) -> PathBuf {
        self.backups_dir().join("profiles")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.root.join(".lock")
    }

    pub fn version_file(&self) -> PathBuf {
        self.root.join(".version")
    }

    pub fn init_file(&self, shell: Shell) -> PathBuf {
        self.root.join(format!("init.{}", shell.init_extension()))
    }

    /// Shortens paths under the home directory to `~/...` for display.
    pub fn pretty(&self, path: &Path) -> String {
        match path.strip_prefix(&self.home) {
            Ok(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
            Err(_) => path.display().to_string(),
        }
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}
