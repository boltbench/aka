//! Checks that keep an alias from breaking the shell or doing something nasty.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::model::{Shell, State};

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

/// Shells where `command` probably won't work, with the reason. Commands are
/// passed to each shell as written, so bash/zsh syntax breaks in PowerShell
/// and PowerShell syntax breaks everywhere else. These are hints, not proof.
pub fn syntax_issues(command: &str) -> Vec<(Shell, &'static str)> {
    let mut issues = Vec::new();
    let words: Vec<&str> = command.split_whitespace().collect();
    let heads = heads(command, false);
    let has = |s: &str| command.contains(s);

    let posix_reason = if heads.iter().any(|w| {
        matches!(
            w.as_str(),
            "export" | "unset" | "source" | "[" | "[[" | "test"
        )
    }) {
        Some("uses bash/zsh builtins like `export` or `[[`")
    } else if words.first().is_some_and(|w| is_assignment(w)) {
        Some("sets a variable with `NAME=value command`")
    } else if has("/dev/null") {
        Some("redirects to /dev/null")
    } else if has("${") || has_posix_variable(command) {
        Some("uses bash-style variables like $USER or ${NAME}")
    } else if has("`") {
        Some("uses backticks")
    } else {
        None
    };
    if let Some(reason) = posix_reason {
        issues.push((Shell::Powershell, reason));
    }
    if has("[[") || has("`") || has("${") {
        issues.push((Shell::Fish, "uses bash/zsh syntax fish doesn't have"));
    }

    // Windows PowerShell maps these names to its own cmdlets, which don't take
    // Unix flags like `ls -la`.
    if let [first, flag, ..] = words.as_slice()
        && WINDOWS_PS_ALIASES.contains(first)
        && flag.starts_with('-')
        && !flag.starts_with("--")
        && flag.len() > 2
    {
        issues.push((
            Shell::Powershell,
            "passes Unix flags to a command PowerShell replaces on Windows",
        ));
    }

    let powershell_only = has("$env:")
        || heads.iter().any(|w| is_cmdlet(w))
        || words.iter().any(|w| w.eq_ignore_ascii_case("-ErrorAction"));
    if powershell_only {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            issues.push((shell, "uses PowerShell syntax"));
        }
    }
    issues.dedup_by(|a, b| a.0 == b.0);
    issues
}

const WINDOWS_PS_ALIASES: &[&str] = &[
    "ls", "rm", "cp", "mv", "cat", "ps", "kill", "sort", "sleep", "curl", "wget", "man", "mount",
];

/// `$USER`, `$PATH` and friends. PowerShell spells these `$env:USER`, but has
/// its own `$HOME`, `$PWD` and `$true`/`$false`/`$null`.
fn has_posix_variable(command: &str) -> bool {
    command.match_indices('$').any(|(i, _)| {
        let name: String = command[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        name.len() > 1
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
            && !matches!(
                name.as_str(),
                "HOME" | "PWD" | "HOST" | "PROFILE" | "PSHOME"
            )
    })
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(var, _)| {
        !var.is_empty() && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// PowerShell's Verb-Noun command names, like `Get-ChildItem`.
fn is_cmdlet(word: &str) -> bool {
    const VERBS: &[&str] = &[
        "Get", "Set", "New", "Remove", "Add", "Clear", "Invoke", "Start", "Stop", "Select",
        "Where", "ForEach", "Out", "Write", "Test", "Copy", "Move", "Import", "Export", "Enter",
    ];
    word.split_once('-').is_some_and(|(verb, noun)| {
        VERBS.contains(&verb) && noun.chars().next().is_some_and(|c| c.is_ascii_uppercase())
    })
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
    fn syntax_that_does_not_travel() {
        let shells = |cmd| {
            syntax_issues(cmd)
                .into_iter()
                .map(|(s, _)| s)
                .collect::<Vec<_>>()
        };
        assert!(shells("git status").is_empty());
        assert!(shells("cd .. && ls").is_empty());
        assert!(shells("echo $HOME").is_empty());
        assert_eq!(shells("export EDITOR=vim"), [Shell::Powershell]);
        assert_eq!(shells("FOO=1 make"), [Shell::Powershell]);
        assert_eq!(shells("echo $USER"), [Shell::Powershell]);
        assert_eq!(shells("which x 2>/dev/null"), [Shell::Powershell]);
        assert_eq!(shells("ls -la"), [Shell::Powershell]);
        assert_eq!(
            shells("[[ -f x ]] && cat x"),
            [Shell::Powershell, Shell::Fish]
        );
        assert_eq!(
            shells("Get-ChildItem -Force"),
            [Shell::Bash, Shell::Zsh, Shell::Fish]
        );
        assert_eq!(
            shells("echo $env:PATH"),
            [Shell::Bash, Shell::Zsh, Shell::Fish]
        );
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
