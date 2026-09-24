//! `aka suggest`: finds long commands you type often and proposes aliases.
//! Everything happens locally; history files are only read, never changed or sent.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::model::State;
use crate::paths::Paths;
use crate::safety;

/// One history file and the shell it belongs to.
pub struct HistoryFile {
    pub shell: &'static str,
    pub path: PathBuf,
}

/// Where each shell keeps its history by default.
pub fn history_files(paths: &Paths) -> Vec<HistoryFile> {
    let home = &paths.home;
    let env_dir = |var: &str| {
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let data = env_dir("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local").join("share"));
    let zdot = env_dir("ZDOTDIR").unwrap_or_else(|| home.clone());

    let mut files = vec![
        HistoryFile {
            shell: "zsh",
            path: zdot.join(".zsh_history"),
        },
        HistoryFile {
            shell: "zsh",
            path: home.join(".histfile"),
        },
        HistoryFile {
            shell: "bash",
            path: home.join(".bash_history"),
        },
        HistoryFile {
            shell: "fish",
            path: data.join("fish").join("fish_history"),
        },
        HistoryFile {
            shell: "powershell",
            path: data
                .join("powershell")
                .join("PSReadLine")
                .join("ConsoleHost_history.txt"),
        },
    ];
    if let Some(appdata) = env_dir("APPDATA") {
        files.push(HistoryFile {
            shell: "powershell",
            path: appdata
                .join("Microsoft")
                .join("Windows")
                .join("PowerShell")
                .join("PSReadLine")
                .join("ConsoleHost_history.txt"),
        });
    }
    // HISTFILE only reaches aka when it's exported, but use it if it is.
    if let Some(custom) = env_dir("HISTFILE") {
        files.push(HistoryFile {
            shell: "zsh/bash",
            path: custom,
        });
    }
    let mut seen = Vec::new();
    files.retain(|f| {
        let real = fs::canonicalize(&f.path).unwrap_or_else(|_| f.path.clone());
        let keep = f.path.is_file() && !seen.contains(&real);
        seen.push(real);
        keep
    });
    files
}

/// Reads the commands out of one history file.
pub fn read_history(file: &HistoryFile) -> Vec<String> {
    let Ok(bytes) = fs::read(&file.path) else {
        return Vec::new();
    };
    match file.shell {
        "zsh" | "zsh/bash" => parse_zsh(&unmetafy(&bytes)),
        "bash" => parse_bash(&String::from_utf8_lossy(&bytes)),
        "fish" => parse_fish(&String::from_utf8_lossy(&bytes)),
        _ => parse_powershell(&String::from_utf8_lossy(&bytes)),
    }
}

/// zsh stores some bytes "metafied": 0x83 followed by the byte XOR 32.
fn unmetafy(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter();
    while let Some(&b) = iter.next() {
        if b == 0x83 {
            if let Some(&next) = iter.next() {
                out.push(next ^ 32);
            }
        } else {
            out.push(b);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Plain lines, or `: <time>:<duration>;command` with EXTENDED_HISTORY.
/// A line ending in a backslash continues on the next one.
fn parse_zsh(text: &str) -> Vec<String> {
    join_continued(text, '\\')
        .into_iter()
        .map(
            |line| match line.strip_prefix(": ").and_then(|l| l.split_once(';')) {
                Some((_, cmd)) => cmd.to_string(),
                None => line,
            },
        )
        .collect()
}

/// Plain lines, skipping the `#<time>` lines HISTTIMEFORMAT adds.
fn parse_bash(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| !(l.starts_with('#') && l[1..].chars().all(|c| c.is_ascii_digit())))
        .map(str::to_string)
        .collect()
}

/// `- cmd: <command>` entries, with `\\` and `\n` escaped.
fn parse_fish(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("- cmd: "))
        .map(|c| c.replace("\\n", "\n").replace("\\\\", "\\"))
        .collect()
}

/// Plain lines; a line ending in a backtick continues on the next one.
fn parse_powershell(text: &str) -> Vec<String> {
    join_continued(text, '`')
}

fn join_continued(text: &str, marker: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if let Some(start) = line.strip_suffix(marker) {
            current.push_str(start);
            current.push('\n');
        } else {
            current.push_str(line);
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub command: String,
    pub count: usize,
    pub name: String,
}

pub struct Options {
    /// Ignore commands used fewer times than this.
    pub min_count: usize,
    /// Show at most this many.
    pub limit: usize,
}

/// Ranks commands by how much typing an alias would save.
pub fn suggest(history: &[String], state: &State, opts: &Options) -> Vec<Suggestion> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for raw in history {
        let cmd = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if !worth_considering(&cmd, raw, state) {
            continue;
        }
        *counts.entry(cmd.clone()).or_default() += 1;
        // `git commit -m "..."` differs every time, but `git commit -m` repeats.
        let words: Vec<&str> = cmd.split(' ').collect();
        for n in 2..=3.min(words.len().saturating_sub(1)) {
            let prefix = &words[..n];
            if prefix
                .iter()
                .any(|w| w.contains(['"', '\'', '$', '|', ';', '&']))
            {
                break;
            }
            *counts.entry(prefix.join(" ")).or_default() += 1;
        }
    }

    let already: Vec<&str> = state
        .aliases
        .aliases
        .values()
        .map(|a| a.command.as_str())
        .collect();
    let mut ranked: Vec<(String, usize)> = counts
        .into_iter()
        .filter(|(cmd, count)| *count >= opts.min_count && cmd.len() >= 8)
        // Already covered: an alias runs this, or a longer or shorter form of it.
        .filter(|(cmd, _)| {
            !already.iter().any(|a| {
                *a == cmd || a.starts_with(&format!("{cmd} ")) || cmd.starts_with(&format!("{a} "))
            })
        })
        .collect();
    // Typing saved: every use of the alias skips roughly the command's length.
    ranked.sort_by(|a, b| {
        (b.1 * b.0.len())
            .cmp(&(a.1 * a.0.len()))
            .then_with(|| a.0.cmp(&b.0))
    });

    let mut picked: Vec<(String, usize)> = Vec::new();
    for (cmd, count) in ranked {
        // Skip near-duplicates: a prefix used about as often as a longer
        // command it starts (or the other way round) adds nothing new.
        let overlaps = picked.iter().any(|(other, other_count)| {
            let related =
                other.starts_with(&format!("{cmd} ")) || cmd.starts_with(&format!("{other} "));
            related && count.abs_diff(*other_count) * 4 <= count.max(*other_count)
        });
        if !overlaps {
            picked.push((cmd, count));
        }
        if picked.len() == opts.limit {
            break;
        }
    }

    let mut taken: Vec<String> = state.aliases.aliases.keys().cloned().collect();
    picked
        .into_iter()
        .map(|(command, count)| {
            let name = pick_name(&command, &taken, |n| safety::shadows(n).is_some());
            taken.push(name.clone());
            Suggestion {
                command,
                count,
                name,
            }
        })
        .collect()
}

/// Filters out things that shouldn't become aliases: one-word commands,
/// multi-line entries, aka itself, existing aliases, and anything that looks
/// like it holds a secret.
fn worth_considering(cmd: &str, raw: &str, state: &State) -> bool {
    if raw.contains('\n') || !cmd.contains(' ') {
        return false;
    }
    let first = cmd.split(' ').next().unwrap_or("");
    if first == "aka" || state.contains(first) {
        return false;
    }
    !looks_secret(cmd)
}

fn looks_secret(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    const WORDS: &[&str] = &[
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "api_key",
        "api-key",
        "bearer ",
        "authorization",
        "private_key",
        "--pass",
    ];
    WORDS.iter().any(|w| lower.contains(w))
        // long random-looking strings are usually keys
        || cmd.split(|c: char| c.is_whitespace() || c == '=' || c == '"' || c == '\'').any(|w| {
            w.len() >= 32 && w.chars().all(|c| c.is_ascii_alphanumeric() || "+/_-".contains(c))
                && w.chars().any(|c| c.is_ascii_digit())
        })
}

/// A short name from the first letters of each word: `git status` → `gs`,
/// `docker compose up -d` → `dcud`. Adds a number if it's taken or would hide
/// a real program.
pub fn pick_name(command: &str, taken: &[String], hides_program: impl Fn(&str) -> bool) -> String {
    let base: String = command
        .split_whitespace()
        .filter_map(|w| {
            w.trim_start_matches('-')
                .chars()
                .find(|c| c.is_ascii_alphanumeric())
        })
        .map(|c| c.to_ascii_lowercase())
        .take(5)
        .collect();
    let base = if base.len() < 2 {
        format!("{base}x")
    } else {
        base
    };
    let usable = |name: &str| {
        safety::validate_name(name).is_ok()
            && !taken.iter().any(|t| t == name)
            && !hides_program(name)
    };
    if usable(&base) {
        return base;
    }
    (2..100)
        .map(|n| format!("{base}{n}"))
        .find(|n| usable(n))
        .unwrap_or_else(|| format!("{base}{}", taken.len() + 100))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Alias;

    fn lines(items: &[(&str, usize)]) -> Vec<String> {
        items
            .iter()
            .flat_map(|(c, n)| std::iter::repeat_n(c.to_string(), *n))
            .collect()
    }

    fn opts() -> Options {
        Options {
            min_count: 3,
            limit: 10,
        }
    }

    #[test]
    fn parses_history_formats() {
        assert_eq!(
            parse_zsh(": 1690000000:0;git status\nls\n: 1690000001:2;echo a \\\nb\n"),
            ["git status", "ls", "echo a \nb"]
        );
        assert_eq!(
            parse_bash("#1690000000\ngit status\n# a comment\n"),
            ["git status", "# a comment"]
        );
        assert_eq!(
            parse_fish("- cmd: git status\n  when: 1690000000\n- cmd: echo a\\\\b\n"),
            ["git status", "echo a\\b"]
        );
        assert_eq!(
            parse_powershell("git status\nGet-ChildItem `\n  -Force\n"),
            ["git status", "Get-ChildItem \n  -Force"]
        );
        assert_eq!(unmetafy(&[b'a', 0x83, b'b' ^ 32, b'c']), "abc");
    }

    #[test]
    fn ranks_by_typing_saved() {
        let history = lines(&[
            ("git status", 20),
            ("docker compose up -d", 10),
            ("ls -la", 50),
            ("echo hi there", 2),
        ]);
        let got = suggest(&history, &State::default(), &opts());
        let commands: Vec<&str> = got.iter().map(|s| s.command.as_str()).collect();
        // `ls -la` is too short to bother with, `echo hi there` too rare
        assert_eq!(commands[0], "docker compose up -d");
        assert!(commands.contains(&"git status"));
        assert!(!commands.contains(&"ls -la"));
        assert!(!commands.iter().any(|c| c.starts_with("echo")));
    }

    #[test]
    fn groups_commands_that_share_a_prefix() {
        let history: Vec<String> = (0..6)
            .map(|i| format!("git commit -m \"fix {i}\""))
            .collect();
        let got = suggest(&history, &State::default(), &opts());
        assert_eq!(got[0].command, "git commit -m");
        assert_eq!(got[0].count, 6);
        assert_eq!(
            got.len(),
            1,
            "the shorter `git commit` prefix is a near-duplicate: {got:?}"
        );
    }

    #[test]
    fn skips_existing_aliases_and_secrets() {
        let mut state = State::default();
        state.insert("gs", Alias::new("git status"));
        state.insert("dcud", Alias::new("docker compose up -d"));
        let history = lines(&[
            ("git status", 10),
            ("git status -sb", 10),
            ("docker compose up -d", 10),
            ("gs -sb", 10),
            ("curl -H 'Authorization: Bearer x' api", 10),
            ("mysql -u root --password=hunter2", 10),
            ("deploy --key 9f8e7d6c5b4a39281706f5e4d3c2b1a0ffee", 10),
        ]);
        assert!(suggest(&history, &state, &opts()).is_empty());
    }

    #[test]
    fn names_from_initials() {
        let none = |_: &str| false;
        assert_eq!(pick_name("git status", &[], none), "gs");
        assert_eq!(pick_name("docker compose up -d", &[], none), "dcud");
        assert_eq!(pick_name("git status", &["gs".into()], none), "gs2");
        // `gs` is also ghostscript on many machines
        assert_eq!(pick_name("git status", &[], |n| n == "gs"), "gs2");
    }
}
