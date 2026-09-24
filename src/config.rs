//! User settings, kept in `config.toml` next to the aliases. `aka config`
//! reads and changes them.

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::paths::Paths;
use crate::store;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub zsh: ZshConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ZshConfig {
    #[serde(default)]
    pub compinit: Compinit,
}

/// How the zsh block turns on completion, when aka is the one turning it on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Compinit {
    /// Checks for new completions on every start (about 30ms).
    #[default]
    Full,
    /// Uses the cached list and checks again once a day (about 5ms).
    Cached,
}

impl Compinit {
    pub fn name(self) -> &'static str {
        match self {
            Compinit::Full => "full",
            Compinit::Cached => "cached",
        }
    }
}

/// One setting `aka config` knows about.
pub struct Setting {
    pub key: &'static str,
    pub about: &'static str,
    pub values: &'static [&'static str],
    get: fn(&Config) -> String,
    set: fn(&mut Config, Option<&str>) -> Result<()>,
}

pub const SETTINGS: &[Setting] = &[Setting {
    key: "zsh.compinit",
    about: "How zsh tab completion starts, when aka turned it on: `full` checks for new \
            completions on every start (~30ms), `cached` checks once a day (~5ms)",
    values: &["full", "cached"],
    get: |c| c.zsh.compinit.name().to_string(),
    set: |c, v| {
        c.zsh.compinit = match v {
            None => Compinit::default(),
            Some(v) => Compinit::from_str(v, true).map_err(|_| {
                anyhow::anyhow!("zsh.compinit must be `full` or `cached`, not `{v}`")
            })?,
        };
        Ok(())
    },
}];

pub fn find(key: &str) -> Result<&'static Setting> {
    SETTINGS.iter().find(|s| s.key == key).with_context(|| {
        let known: Vec<&str> = SETTINGS.iter().map(|s| s.key).collect();
        format!(
            "there's no setting called `{key}`. Settings: {}",
            known.join(", ")
        )
    })
}

impl Setting {
    pub fn get(&self, config: &Config) -> String {
        (self.get)(config)
    }

    pub fn is_default(&self, config: &Config) -> bool {
        self.get(config) == self.get(&Config::default())
    }

    /// Sets the value, or resets it to the default with `None`.
    pub fn set(&self, config: &mut Config, value: Option<&str>) -> Result<()> {
        (self.set)(config, value)
    }
}

pub fn load(paths: &Paths) -> Result<Config> {
    match std::fs::read_to_string(paths.config_file()) {
        Ok(text) => toml::from_str(&text).with_context(|| {
            format!(
                "{} isn't valid. Fix it by hand or delete it to go back to the defaults",
                paths.config_file().display()
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("couldn't read the config file"),
    }
}

pub fn save(paths: &Paths, config: &Config) -> Result<()> {
    if *config == Config::default() {
        return store::remove_if_exists(&paths.config_file());
    }
    let text = format!(
        "# aka settings. Change them with `aka config set <key> <value>`.\n\n{}",
        toml::to_string_pretty(config)?
    );
    store::write_atomic(&paths.config_file(), &text)
}

/// Checks a value without saving it, for friendlier errors before any work.
pub fn validate(key: &str, value: &str) -> Result<()> {
    let setting = find(key)?;
    if !setting.values.is_empty() && !setting.values.contains(&value) {
        bail!(
            "`{key}` can be {}, not `{value}`",
            setting.values.join(" or ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_and_resets() {
        let mut config = Config::default();
        let s = find("zsh.compinit").unwrap();
        assert!(s.is_default(&config));
        s.set(&mut config, Some("cached")).unwrap();
        assert_eq!(s.get(&config), "cached");
        assert!(s.set(&mut config, Some("sometimes")).is_err());
        s.set(&mut config, None).unwrap();
        assert!(s.is_default(&config));
        assert!(find("nope").is_err());
        assert!(validate("zsh.compinit", "fast").is_err());
    }

    #[test]
    fn round_trips() {
        let mut config = Config::default();
        config.zsh.compinit = Compinit::Cached;
        let text = toml::to_string_pretty(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&text).unwrap(), config);
        assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    }
}
