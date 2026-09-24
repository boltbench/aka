use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::engine::{ArgValueCandidates, CompletionCandidate};

use crate::model::{Os, Shell};
use crate::paths::Paths;
use crate::store;

#[derive(Debug, Parser)]
#[command(
    name = "aka",
    version,
    about = "A.K.A (also known as): manage your shell aliases from one place",
    long_about = "A.K.A (also known as): manage your shell aliases from one place.\n\n\
        Aliases live in one file and work in bash, zsh, fish and PowerShell.\n\
        Run `aka setup` once, then `aka add gs \"git status\"`.",
    after_help = "Run `aka` with no command to list your aliases.",
    max_term_width = 100
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Args)]
pub struct GlobalArgs {
    /// Answer yes to every question
    #[arg(short, long, global = true)]
    pub yes: bool,

    /// Skip safety questions and override locks
    #[arg(short, long, global = true)]
    pub force: bool,

    /// Show what would change without saving anything
    #[arg(long, global = true)]
    pub dry_run: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add an alias, e.g. `aka add gs git status`
    #[command(
        visible_alias = "set",
        after_help = "Put options before the command. Everything after the name is treated as the command."
    )]
    Add(AddArgs),

    /// Remove aliases (they go to the trash, see `aka restore`)
    #[command(visible_aliases = ["remove", "delete"])]
    Rm {
        #[arg(required = true, add = alias_names())]
        names: Vec<String>,
        /// Delete for good instead of moving to the trash
        #[arg(long)]
        purge: bool,
    },

    /// Bring back a removed alias, or list the trash
    Restore {
        #[arg(add = trash_names())]
        name: Option<String>,
    },

    /// List your aliases
    #[command(visible_alias = "ls")]
    List(ListArgs),

    /// Show everything about one alias
    Show {
        #[arg(add = alias_names())]
        name: String,
    },

    /// Change an alias's name
    #[command(visible_alias = "mv")]
    Rename {
        #[arg(add = alias_names())]
        old: String,
        new: String,
    },

    /// Copy an alias under a new name
    #[command(visible_alias = "copy")]
    Cp {
        #[arg(add = alias_names())]
        source: String,
        target: String,
    },

    /// Change an alias, or open the whole alias file in your editor
    Edit {
        #[arg(add = alias_names())]
        name: Option<String>,
    },

    /// Turn aliases back on
    Enable {
        #[arg(required_unless_present = "tag", add = alias_names())]
        names: Vec<String>,
        /// Every alias with this tag
        #[arg(long, add = tag_names())]
        tag: Option<String>,
    },

    /// Turn aliases off without deleting them
    Disable {
        #[arg(required_unless_present = "tag", add = alias_names())]
        names: Vec<String>,
        /// Every alias with this tag
        #[arg(long, add = tag_names())]
        tag: Option<String>,
    },

    /// Add tags to an alias
    Tag {
        #[arg(add = alias_names())]
        name: String,
        #[arg(required = true, add = tag_names())]
        tags: Vec<String>,
    },

    /// Remove tags from an alias
    Untag {
        #[arg(add = alias_names())]
        name: String,
        #[arg(required = true, add = tag_names())]
        tags: Vec<String>,
    },

    /// List your tags
    Tags,

    /// Suggest aliases for long commands you type often (reads your shell history locally)
    Suggest {
        /// How many suggestions to show
        #[arg(short = 'n', long, default_value_t = 10)]
        limit: usize,
        /// Only commands used at least this many times
        #[arg(long, default_value_t = 3)]
        min_count: usize,
    },

    /// Protect aliases from being replaced, renamed or removed
    Lock {
        #[arg(required = true, add = alias_names())]
        names: Vec<String>,
    },

    /// Remove the protection added by `aka lock`
    Unlock {
        #[arg(required = true, add = alias_names())]
        names: Vec<String>,
    },

    /// Revert the last change
    Undo,

    /// Show recent changes
    History {
        /// How many entries to show
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: usize,
    },

    /// Hook aka into your shell profiles (run this once)
    Setup {
        /// Only set up these shells (default: every installed shell)
        #[arg(long = "shell", value_delimiter = ',')]
        shells: Vec<Shell>,
        /// Don't offer to turn on zsh tab completion
        #[arg(long)]
        no_completion: bool,
    },

    /// Remove the aka hook from your shell profiles
    Uninstall {
        /// Only these shells (default: all)
        #[arg(long = "shell", value_delimiter = ',')]
        shells: Vec<Shell>,
        /// Also delete your aliases, trash and backups
        #[arg(long)]
        purge: bool,
    },

    /// Print the init script for a shell (for `eval "$(aka init zsh)"` style setups)
    Init { shell: Shell },

    /// Bring in aliases that are already defined in your shell profiles
    Import(ImportArgs),

    /// Check that everything is set up and healthy
    Doctor,

    /// Show or change settings
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print one setting
    Get {
        #[arg(add = setting_keys())]
        key: String,
    },
    /// Change a setting
    Set {
        #[arg(add = setting_keys())]
        key: String,
        value: String,
    },
    /// Put a setting back to its default
    Unset {
        #[arg(add = setting_keys())]
        key: String,
    },
}

fn setting_keys() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        crate::config::SETTINGS
            .iter()
            .map(|s| CompletionCandidate::new(s.key).help(Some(s.values.join(" | ").into())))
            .collect()
    })
}

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Name of the alias
    pub name: String,

    /// The command it runs. Quote it, or just type it after the name
    #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true, num_args = 1..)]
    pub command: Vec<String>,

    #[command(flatten)]
    pub opts: AddOpts,
}

/// Options for `aka add`. Kept separate so they can also be read when they
/// come after a quoted command: `aka add gs "git status" -d "quick status"`.
#[derive(Debug, Default, Args)]
pub struct AddOpts {
    /// A short note shown in `aka list`
    #[arg(short, long)]
    pub description: Option<String>,

    /// Group it under these tags, e.g. --tag git
    #[arg(short, long = "tag", value_delimiter = ',', add = tag_names())]
    pub tags: Vec<String>,

    /// Only define it in these shells
    #[arg(long = "shell", value_delimiter = ',')]
    pub shells: Vec<Shell>,

    /// Only define it on these systems
    #[arg(long = "os", value_delimiter = ',')]
    pub os: Vec<Os>,

    /// Protect it from being replaced or removed
    #[arg(long)]
    pub lock: bool,

    /// Ask "run this?" every time it's used
    #[arg(long)]
    pub confirm: bool,
}

/// Options that can follow a quoted command: `aka add gs "git status" -d note`.
#[derive(Parser)]
#[command(name = "aka add", no_binary_name = true)]
struct Trailing {
    #[command(flatten)]
    opts: AddOpts,
    #[command(flatten)]
    global: GlobalArgs,
}

impl AddArgs {
    /// Splits `command` into the command itself and any options typed after it.
    /// Only a quoted command (one argument containing a space) can be followed by
    /// options; otherwise every word belongs to the command, so `aka add ll ls -d`
    /// keeps its `-d`.
    /// Returns any global flags (`-y`, `-f`, `--dry-run`) found there too.
    pub fn split_trailing_options(&mut self) -> Result<Option<GlobalArgs>, clap::Error> {
        if self.command.len() < 2 || !self.command[0].contains(char::is_whitespace) {
            return Ok(None);
        }
        let rest = self.command.split_off(1);
        let trailing = Trailing::try_parse_from(rest)?;
        let t = trailing.opts;
        let o = &mut self.opts;
        o.description = t.description.or(o.description.take());
        o.tags.extend(t.tags);
        o.shells.extend(t.shells);
        o.os.extend(t.os);
        o.lock |= t.lock;
        o.confirm |= t.confirm;
        Ok(Some(trailing.global))
    }

    /// For an unquoted command, the trailing words that look like they were
    /// meant as aka's own options (`aka add g git -d "note"`). They stay part of
    /// the command, since `docker run -d nginx` needs its `-d`, but it's worth a
    /// heads-up. Returns how many trailing words look that way.
    pub fn misplaced_options(&self) -> Option<usize> {
        if self.command.len() < 2 || self.command[0].contains(char::is_whitespace) {
            return None;
        }
        (1..self.command.len()).find_map(|start| {
            let tail = &self.command[start..];
            if !tail[0].starts_with('-') {
                return None;
            }
            let parsed = Trailing::try_parse_from(tail).ok()?;
            // Only speak up on strong signals: a description with spaces in it,
            // or one of aka's long options. `-d nginx` alone is far more likely
            // to be the command's own flag.
            let long_option = tail.iter().any(|t| {
                [
                    "--description",
                    "--tag",
                    "--shell",
                    "--os",
                    "--lock",
                    "--confirm",
                ]
                .iter()
                .any(|f| t == f || t.starts_with(&format!("{f}=")))
            });
            let sentence = parsed
                .opts
                .description
                .as_deref()
                .is_some_and(|d| d.contains(char::is_whitespace));
            (long_option || sentence).then_some(tail.len())
        })
    }
}

#[derive(Debug, Args)]
pub struct ListArgs {
    /// Only show aliases whose name, command or description contains this
    pub filter: Option<String>,

    /// Only show aliases with this tag
    #[arg(long, add = tag_names())]
    pub tag: Option<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Table)]
    pub format: Format,

    /// Same as --format json
    #[arg(long, conflicts_with = "format")]
    pub json: bool,

    /// Same as --format plain (name, tab, command)
    #[arg(long, conflicts_with_all = ["format", "json"])]
    pub plain: bool,

    /// Only print names, one per line
    #[arg(long, hide = true)]
    pub names: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Table,
    Plain,
    Json,
}

#[derive(Debug, Args)]
pub struct ImportArgs {
    /// Read these files instead of the usual shell profiles
    #[arg(long = "from")]
    pub files: Vec<PathBuf>,

    /// What to do with the original lines: delete, comment or keep (asks if not given)
    #[arg(long, value_enum)]
    pub clean: Option<Clean>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Clean {
    Delete,
    Comment,
    Keep,
}

fn alias_names() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Some(state) = Paths::resolve().ok().and_then(|p| store::load(&p).ok()) else {
            return Vec::new();
        };
        state
            .aliases
            .aliases
            .into_iter()
            .map(|(name, alias)| {
                let help = alias.description.unwrap_or(alias.command);
                CompletionCandidate::new(name).help(Some(help.into()))
            })
            .collect()
    })
}

fn tag_names() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Some(state) = Paths::resolve().ok().and_then(|p| store::load(&p).ok()) else {
            return Vec::new();
        };
        let mut tags: Vec<String> = state
            .aliases
            .aliases
            .into_values()
            .flat_map(|a| a.tags)
            .collect();
        tags.sort();
        tags.dedup();
        tags.into_iter().map(CompletionCandidate::new).collect()
    })
}

fn trash_names() -> ArgValueCandidates {
    ArgValueCandidates::new(|| {
        let Some(state) = Paths::resolve().ok().and_then(|p| store::load(&p).ok()) else {
            return Vec::new();
        };
        state
            .trash
            .trash
            .into_iter()
            .filter_map(|(name, mut versions)| {
                let newest = versions.pop()?;
                Some(CompletionCandidate::new(name).help(Some(newest.alias.command.into())))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn add_takes_unquoted_commands() {
        let cli = Cli::try_parse_from(["aka", "add", "-d", "list all", "ll", "ls", "-la"]).unwrap();
        let Some(Command::Add(mut args)) = cli.command else {
            panic!()
        };
        args.split_trailing_options().unwrap();
        assert_eq!(args.name, "ll");
        assert_eq!(args.command, ["ls", "-la"]);
        assert_eq!(args.opts.description.as_deref(), Some("list all"));
    }

    #[test]
    fn options_after_a_quoted_command() {
        let cli = Cli::try_parse_from([
            "aka",
            "add",
            "gs",
            "git status",
            "-d",
            "quick",
            "--lock",
            "-y",
        ])
        .unwrap();
        let Some(Command::Add(mut args)) = cli.command else {
            panic!()
        };
        let global = args.split_trailing_options().unwrap().unwrap();
        assert!(global.yes);
        assert_eq!(args.command, ["git status"]);
        assert_eq!(args.opts.description.as_deref(), Some("quick"));
        assert!(args.opts.lock);

        let cli = Cli::try_parse_from(["aka", "add", "g", "git", "-d", "git shortcut"]).unwrap();
        let Some(Command::Add(args)) = cli.command else {
            panic!()
        };
        assert_eq!(args.misplaced_options(), Some(2));
        let Some(Command::Add(args)) = Cli::try_parse_from(["aka", "add", "g", "git", "--lock"])
            .unwrap()
            .command
        else {
            panic!()
        };
        assert_eq!(args.misplaced_options(), Some(1));
        for fine in [
            &["docker", "run", "-d", "nginx"][..],
            &["ls", "-la"],
            &["apt", "upgrade", "-y"],
        ] {
            let mut argv = vec!["aka", "add", "x"];
            argv.extend_from_slice(fine);
            let Some(Command::Add(args)) = Cli::try_parse_from(argv).unwrap().command else {
                panic!()
            };
            assert_eq!(args.misplaced_options(), None, "{fine:?}");
        }

        let cli = Cli::try_parse_from(["aka", "add", "gs", "git status", "--bogus"]).unwrap();
        let Some(Command::Add(mut args)) = cli.command else {
            panic!()
        };
        assert!(args.split_trailing_options().is_err());
    }
}
