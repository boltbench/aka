//! Checks that keep an alias from breaking the shell or doing something nasty.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::model::State;

const MAX_NAME_LEN: usize = 64;

/// Names an alias can't take. Shell keywords, plus the commands aka's own
/// shell hook runs: bash and zsh expand aliases while reading a function, so
/// aliasing `.` or `command` would break the hook and every reload after it.
const RESERVED: &[&str] = &[
    // aka and the commands its hook relies on
    "aka",
    ".",
    "source",
    "command",
    "builtin",
    "printf",
    "read",
    "local",
    "return",
    "unalias",
    "unset",
    "unfunction",
    "complete",
    "compdef",
    "eval",
    "exec",
    // shell keywords
    "if",
    "then",
    "else",
    "elif",
    "fi",
    "for",
    "while",
    "until",
    "do",
    "done",
    "case",
    "esac",
    "in",
    "function",
    "select",
    "time",
    "coproc",
    "begin",
    "end",
    "switch",
    "not",
    "and",
    "or",
    "foreach",
    "repeat",
    "break",
    "continue",
    "exit",
    "set",
    "alias",
];

/// Builtins in bash, zsh, fish or PowerShell that an alias would quietly override.
const BUILTINS: &[&str] = &[
    "cd",
    "echo",
    "export",
    "pwd",
    "read",
    "test",
    "true",
    "false",
    "type",
    "command",
    "builtin",
    "declare",
    "typeset",
    "history",
    "jobs",
    "fg",
    "bg",
    "kill",
    "wait",
    "trap",
    "umask",
    "ulimit",
    "shift",
    "printf",
    "hash",
    "help",
    "let",
    "pushd",
    "popd",
    "dirs",
    "bind",
    "complete",
    "compgen",
    "shopt",
    "enable",
    "logout",
    "readonly",
    "times",
    "getopts",
    "disown",
    "suspend",
    "whence",
    "where",
    "which",
    "functions",
    "autoload",
    "bindkey",
    "emulate",
    "setopt",
    "unsetopt",
    "abbr",
    "contains",
    "count",
    "math",
    "string",
    "status",
    "commandline",
    "cls",
    "clear",
    "dir",
    "del",
    "copy",
    "move",
    "ren",
    "start",
];

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("an alias name can't be empty");
    }
    if name.len() > MAX_NAME_LEN {
        bail!("`{name}` is too long (the limit is {MAX_NAME_LEN} characters)");
    }
    if name.starts_with('-') {
        bail!("`{name}` can't start with a dash, it would look like an option");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        let shown = if bad.is_whitespace() {
            "a space".to_string()
        } else {
            format!("`{bad}`")
        };
        bail!("`{name}` contains {shown}. Use letters, numbers, `_`, `-` or `.`");
    }
    if name.chars().all(|c| c.is_ascii_digit()) {
        bail!("`{name}` is only digits, which shells don't allow as a name");
    }
    if RESERVED.contains(&name) {
        bail!("`{name}` is reserved: it's a shell keyword or a command aka's shell hook needs");
    }
    Ok(())
}

pub fn validate_command(command: &str) -> Result<()> {
    if command.trim().is_empty() {
        bail!("the command can't be empty");
    }
    if command.contains('\n') || command.contains('\r') {
        bail!("the command has to fit on one line (join multiple steps with `&&` or `;`)");
    }
    Ok(())
}

/// What an alias named `name` would hide, if anything.
pub enum Shadow {
    Builtin,
    Program(PathBuf),
}

impl std::fmt::Display for Shadow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Shadow::Builtin => f.write_str("a shell builtin"),
            Shadow::Program(path) => write!(f, "the program at {}", path.display()),
        }
    }
}

pub fn shadows(name: &str) -> Option<Shadow> {
    if BUILTINS.contains(&name) {
        return Some(Shadow::Builtin);
    }
    which::which(name).ok().map(Shadow::Program)
}

/// True when the alias just adds options to the command it's named after, like `ls='ls -G'`.
/// Hiding the original is the whole point there, so it isn't worth a warning.
pub fn is_wrapper(name: &str, command: &str) -> bool {
    first_words(command).iter().any(|w| w == name)
}

/// Reasons a command looks destructive. Empty when it looks fine.
pub fn danger(command: &str) -> Vec<&'static str> {
    let c = command.to_lowercase();
    let squashed = c.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut reasons = Vec::new();
    let mut check = |hit: bool, reason| {
        if hit && !reasons.contains(&reason) {
            reasons.push(reason);
        }
    };

    check(
        [
            "rm -rf",
            "rm -fr",
            "rm -r -f",
            "rm -f -r",
            "rm --recursive --force",
        ]
        .iter()
        .any(|p| squashed.contains(p))
            || (squashed.contains("remove-item")
                && squashed.contains("-recurse")
                && squashed.contains("-force")),
        "deletes files recursively without asking",
    );
    check(
        heads(&squashed, false)
            .iter()
            .any(|w| w == "sudo" || w == "doas"),
        "runs with admin rights",
    );
    check(
        [
            "| sh",
            "|sh",
            "| bash",
            "|bash",
            "| zsh",
            "|zsh",
            "| iex",
            "|iex",
            "invoke-expression",
        ]
        .iter()
        .any(|p| squashed.contains(p)),
        "pipes text straight into a shell",
    );
    check(
        squashed.contains("mkfs")
            || squashed.contains("of=/dev/")
            || squashed.contains("> /dev/sd"),
        "can overwrite a disk",
    );
    check(
        squashed.contains("chmod -r 777"),
        "makes files writable by everyone",
    );
    check(squashed.contains(":(){"), "looks like a fork bomb");
    check(
        squashed.contains("git reset --hard") || squashed.contains("git clean -f"),
        "throws away uncommitted work",
    );
    check(
        squashed.contains("push --force") || squashed.contains("push -f"),
        "force-pushes over remote history",
    );
    reasons
}

/// If adding `name = command` would make aliases call each other in a circle,
/// returns the cycle, e.g. `["a", "b", "a"]`.
pub fn find_loop(state: &State, name: &str, command: &str) -> Option<Vec<String>> {
    let mut graph: BTreeMap<&str, Vec<String>> = state
        .aliases
        .aliases
        .iter()
        .map(|(n, a)| (n.as_str(), first_words(&a.command)))
        .collect();
    graph.insert(name, first_words(command));

    let mut path = vec![name.to_string()];
    let mut seen = HashSet::new();
    if walk(&graph, name, name, &mut path, &mut seen) {
        Some(path)
    } else {
        None
    }
}

fn walk(
    graph: &BTreeMap<&str, Vec<String>>,
    start: &str,
    current: &str,
    path: &mut Vec<String>,
    seen: &mut HashSet<String>,
) -> bool {
    let Some(next) = graph.get(current) else {
        return false;
    };
    for word in next {
        // An alias that calls the command it's named after (`ls='ls -G'`) isn't a loop.
        if word == current || !graph.contains_key(word.as_str()) {
            continue;
        }
        path.push(word.clone());
        if word == start {
            return true;
        }
        if seen.insert(word.clone()) && walk(graph, start, word, path, seen) {
            return true;
        }
        path.pop();
    }
    false
}

/// The word in command position of each step: `a && b | c; d` gives a, b, c, d.
/// Skips `VAR=value` prefixes and wrappers like `sudo` or `command`.
pub fn first_words(command: &str) -> Vec<String> {
    heads(command, true)
}

fn heads(command: &str, skip_wrappers: bool) -> Vec<String> {
    let mut words = Vec::new();
    for segment in command.split(['|', '&', ';', '(', ')', '{', '}']) {
        let word = segment.split_whitespace().find(|w| {
            !w.contains('=')
                && !(skip_wrappers
                    && matches!(
                        *w,
                        "sudo" | "doas" | "command" | "exec" | "builtin" | "nohup" | "time"
                    ))
        });
        if let Some(w) = word {
            let w = w.trim_matches(['"', '\'']);
            if !w.is_empty() && !words.iter().any(|x| x == w) {
                words.push(w.to_string());
            }
        }
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Alias;

    #[test]
    fn names() {
        for ok in ["gs", "ll", "git-st", "k8s", "dc.up", "_x", "..", "..."] {
            assert!(validate_name(ok).is_ok(), "{ok} should be allowed");
        }
        for bad in [
            "",
            "-x",
            "has space",
            "a=b",
            "semi;colon",
            "if",
            "aka",
            ".",
            "command",
            "123",
            "a/b",
            "$x",
        ] {
            assert!(validate_name(bad).is_err(), "{bad} should be rejected");
        }
        assert!(validate_name(&"x".repeat(65)).is_err());
    }

    #[test]
    fn commands() {
        assert!(validate_command("git status").is_ok());
        assert!(validate_command("  ").is_err());
        assert!(validate_command("a\nb").is_err());
    }

    #[test]
    fn dangerous_commands() {
        assert!(danger("git status").is_empty());
        assert!(danger("rm -rf build").contains(&"deletes files recursively without asking"));
        assert!(danger("rm   -fr  build").contains(&"deletes files recursively without asking"));
        assert!(danger("sudo apt update").contains(&"runs with admin rights"));
        assert!(
            danger("curl -fsSL example.com/x | sh").contains(&"pipes text straight into a shell")
        );
        assert!(
            danger("Remove-Item -Recurse -Force .\\bin")
                .contains(&"deletes files recursively without asking")
        );
        assert!(danger("git reset --hard HEAD").contains(&"throws away uncommitted work"));
        // `sudo` only counts in command position
        assert!(danger("echo pseudo sudoku").is_empty());
    }

    #[test]
    fn command_words() {
        assert_eq!(first_words("git status"), vec!["git"]);
        assert_eq!(
            first_words("cd .. && ls -la | less"),
            vec!["cd", "ls", "less"]
        );
        assert_eq!(first_words("FOO=1 sudo make install"), vec!["make"]);
    }

    #[test]
    fn wrappers() {
        assert!(is_wrapper("ls", "ls -G"));
        assert!(is_wrapper("grep", "grep --color=auto"));
        assert!(!is_wrapper("ls", "eza -la"));
    }

    fn state(pairs: &[(&str, &str)]) -> State {
        let mut s = State::default();
        for (n, c) in pairs {
            s.insert(*n, Alias::new(*c));
        }
        s
    }

    #[test]
    fn loops() {
        let s = state(&[("a", "b --x"), ("b", "c")]);
        assert_eq!(
            find_loop(&s, "c", "a"),
            Some(vec!["c".into(), "a".into(), "b".into(), "c".into()])
        );
        assert_eq!(find_loop(&s, "c", "echo hi"), None);
        // self reference is how wrappers work, not a loop
        assert_eq!(find_loop(&s, "ls", "ls -G"), None);
        // direct two-step loop
        let s = state(&[("x", "y")]);
        assert_eq!(
            find_loop(&s, "y", "x && echo"),
            Some(vec!["y".into(), "x".into(), "y".into()])
        );
    }
}
