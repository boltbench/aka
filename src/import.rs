//! `aka import`: reads aliases out of existing shell profiles, adds them to aka,
//! then offers to delete or comment out the original lines.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::{Clean, ImportArgs};
use crate::context::Ctx;
use crate::model::{Alias, Os, Shell, State};
use crate::prompt::{self, Resolution};
use crate::{safety, setup, store, ui};

/// Which syntax a file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syntax {
    Posix,
    Fish,
    Powershell,
}

impl Syntax {
    fn for_path(path: &Path) -> Syntax {
        let name = path.to_string_lossy().to_lowercase();
        if name.ends_with(".fish") {
            Syntax::Fish
        } else if name.ends_with(".ps1") {
            Syntax::Powershell
        } else {
            Syntax::Posix
        }
    }

    /// Shells an imported alias should apply to. Commands written for one
    /// shell family often don't work in another, so stay within the family.
    fn shells(self) -> Vec<Shell> {
        match self {
            Syntax::Posix => vec![Shell::Bash, Shell::Zsh],
            Syntax::Fish => vec![Shell::Fish],
            Syntax::Powershell => vec![Shell::Powershell],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    pub command: String,
    /// 0-based line index in the file.
    pub line: usize,
    /// True when the line holds exactly this one alias at the top level, so
    /// it's safe to delete or comment out.
    pub cleanable: bool,
    /// The line as it was read. Cleanup only touches a line that still matches.
    pub raw: String,
}

pub fn parse(text: &str, syntax: Syntax) -> (Vec<Found>, Vec<String>) {
    let mut found = Vec::new();
    let mut skipped = Vec::new();
    let mut in_aka_block = false;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.contains(setup::START) {
            in_aka_block = true;
        }
        if line.contains(setup::END) {
            in_aka_block = false;
            continue;
        }
        if in_aka_block {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let result = match syntax {
            Syntax::Posix => parse_posix(line.trim_start()),
            Syntax::Fish => parse_fish(line.trim_start()),
            Syntax::Powershell => parse_powershell(line.trim_start()),
        };
        match result {
            Parsed::None => {}
            Parsed::Skip(why) => skipped.push(format!("line {}: {why}", i + 1)),
            Parsed::Aliases(defs, rest_empty) => {
                let single = defs.len() == 1 && rest_empty && !indented;
                for (name, command) in defs {
                    found.push(Found {
                        name,
                        command,
                        line: i,
                        cleanable: single,
                        raw: line.to_string(),
                    });
                }
            }
        }
    }
    (found, skipped)
}

enum Parsed {
    None,
    Skip(String),
    /// The aliases on the line, and whether nothing else follows them.
    Aliases(Vec<(String, String)>, bool),
}

fn parse_posix(line: &str) -> Parsed {
    let Some(rest) = line.strip_prefix("alias") else {
        return Parsed::None;
    };
    if !rest.starts_with([' ', '\t']) {
        return Parsed::None;
    }
    let mut words = Words::new(rest, Quoting::Posix);
    let mut defs = Vec::new();
    while let Some(word) = words.next_word() {
        if word == "--" {
            continue;
        }
        if word.starts_with('-') {
            if word.contains('g') || word.contains('s') {
                return Parsed::Skip(
                    "global or suffix aliases (alias -g / -s) aren't supported".into(),
                );
            }
            continue;
        }
        let Some((name, command)) = word.split_once('=') else {
            // `alias ll` just prints an alias
            return Parsed::None;
        };
        defs.push((name.to_string(), command.to_string()));
    }
    if defs.is_empty() {
        return Parsed::None;
    }
    Parsed::Aliases(defs, words.rest_is_empty())
}

fn parse_fish(line: &str) -> Parsed {
    let (keyword, rest) = match line.split_once([' ', '\t']) {
        Some(pair) => pair,
        None => return Parsed::None,
    };
    if keyword != "alias" && keyword != "abbr" {
        return Parsed::None;
    }
    let mut words = Words::new(rest, Quoting::Fish);
    let mut args = Vec::new();
    while let Some(word) = words.next_word() {
        if word.starts_with('-') && args.is_empty() {
            // abbr --add / alias --save; other abbr modes can't become aliases
            if keyword == "abbr"
                && !matches!(
                    word.as_str(),
                    "-a" | "--add" | "-g" | "--global" | "-U" | "--universal"
                )
            {
                return Parsed::Skip(format!("`abbr {word}` isn't an alias"));
            }
            continue;
        }
        args.push(word);
    }
    let (name, command) = match args.as_slice() {
        [single] => match single.split_once('=') {
            Some((n, c)) => (n.to_string(), c.to_string()),
            None => return Parsed::None,
        },
        [name, rest @ ..] if !rest.is_empty() => match name.split_once('=') {
            Some((n, c)) => (
                n.to_string(),
                std::iter::once(c.to_string())
                    .chain(rest.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            None => (name.clone(), rest.join(" ")),
        },
        _ => return Parsed::None,
    };
    Parsed::Aliases(vec![(name, command)], words.rest_is_empty())
}

fn parse_powershell(line: &str) -> Parsed {
    let (keyword, rest) = match line.split_once([' ', '\t']) {
        Some(pair) => pair,
        None => return Parsed::None,
    };
    if !["set-alias", "new-alias", "sal", "nal"].contains(&keyword.to_lowercase().as_str()) {
        return Parsed::None;
    }
    let mut words = Words::new(rest, Quoting::Powershell);
    let mut name = None;
    let mut value = None;
    let mut positional = Vec::new();
    while let Some(word) = words.next_word() {
        let lower = word.to_lowercase();
        if lower.starts_with('-') {
            match lower.as_str() {
                "-name" | "-n" => name = words.next_word(),
                "-value" | "-v" => value = words.next_word(),
                "-option" | "-scope" | "-description" => {
                    words.next_word();
                }
                _ => {}
            }
            continue;
        }
        positional.push(word);
    }
    let mut positional = positional.into_iter();
    let name = name.or_else(|| positional.next());
    let value = value.or_else(|| positional.next());
    match (name, value) {
        (Some(n), Some(v)) => Parsed::Aliases(vec![(n, v)], words.rest_is_empty()),
        _ => Parsed::None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Quoting {
    Posix,
    Fish,
    Powershell,
}

/// A small shell-word splitter: handles the quoting each shell uses and stops
/// at a comment or a command separator.
struct Words<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    text: &'a str,
    quoting: Quoting,
    stopped_at: Option<usize>,
}

impl<'a> Words<'a> {
    fn new(text: &'a str, quoting: Quoting) -> Self {
        Self {
            chars: text.char_indices().peekable(),
            text,
            quoting,
            stopped_at: None,
        }
    }

    fn next_word(&mut self) -> Option<String> {
        if self.stopped_at.is_some() {
            return None;
        }
        while self.chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            self.chars.next();
        }
        let &(start, first) = self.chars.peek()?;
        if first == '#' || first == ';' || first == '&' || first == '|' {
            self.stopped_at = Some(start);
            return None;
        }
        let mut word = String::new();
        while let Some(&(i, c)) = self.chars.peek() {
            if c.is_whitespace() {
                break;
            }
            if matches!(c, ';' | '&' | '|') {
                self.stopped_at = Some(i);
                break;
            }
            self.chars.next();
            match (c, self.quoting) {
                ('\'', Quoting::Posix) => {
                    for (_, c) in self.chars.by_ref() {
                        if c == '\'' {
                            break;
                        }
                        word.push(c);
                    }
                }
                ('\'', Quoting::Fish) => {
                    while let Some((_, c)) = self.chars.next() {
                        match c {
                            '\'' => break,
                            '\\' if matches!(self.chars.peek(), Some((_, '\'' | '\\'))) => {
                                word.push(self.chars.next().unwrap().1);
                            }
                            _ => word.push(c),
                        }
                    }
                }
                ('\'', Quoting::Powershell) => {
                    while let Some((_, c)) = self.chars.next() {
                        if c == '\'' {
                            if matches!(self.chars.peek(), Some((_, '\''))) {
                                self.chars.next();
                                word.push('\'');
                                continue;
                            }
                            break;
                        }
                        word.push(c);
                    }
                }
                ('"', q) => {
                    let escape = if q == Quoting::Powershell { '`' } else { '\\' };
                    while let Some((_, c)) = self.chars.next() {
                        if c == '"' {
                            break;
                        }
                        if c == escape
                            && let Some(&(_, next)) = self.chars.peek()
                            && (q == Quoting::Powershell || matches!(next, '"' | '\\' | '$' | '`'))
                        {
                            word.push(next);
                            self.chars.next();
                            continue;
                        }
                        word.push(c);
                    }
                }
                ('\\', Quoting::Posix | Quoting::Fish) => {
                    if let Some((_, next)) = self.chars.next() {
                        word.push(next);
                    }
                }
                _ => word.push(c),
            }
        }
        Some(word)
    }

    /// True if nothing but whitespace or a comment followed the last word.
    fn rest_is_empty(&mut self) -> bool {
        while self.next_word().is_some() {}
        match self.stopped_at {
            None => true,
            Some(i) => self.text[i..].starts_with('#'),
        }
    }
}

struct Source {
    path: PathBuf,
    syntax: Syntax,
    found: Vec<Found>,
}

pub fn run(ctx: &Ctx, args: ImportArgs) -> Result<()> {
    let candidates: Vec<PathBuf> = if args.files.is_empty() {
        Shell::ALL
            .into_iter()
            .flat_map(|shell| setup::import_profiles(shell, &ctx.paths))
            .filter(|f| f.exists())
            .collect()
    } else {
        args.files.clone()
    };
    // One file can be reachable under several names (`.bash_profile` symlinked
    // to `.profile`, or the same --from twice). Reading it twice would make the
    // cleanup edit it twice, so keep only the first name for each real file.
    let mut seen = Vec::new();
    let files: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|f| {
            let real = fs::canonicalize(f).unwrap_or_else(|_| f.clone());
            let new = !seen.contains(&real);
            seen.push(real);
            new
        })
        .collect();

    let mut sources = Vec::new();
    for path in files {
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                ui::warn(format!("couldn't read {}: {e}", ctx.paths.pretty(&path)));
                continue;
            }
        };
        let syntax = Syntax::for_path(&path);
        let (found, skipped) = parse(&text, syntax);
        for f in found.iter().filter(|f| !f.cleanable) {
            ui::hint(format!(
                "{}: line {} ({}) is inside a block or shares its line, so it stays in the file. aka will load it every time.",
                ctx.paths.pretty(&path),
                f.line + 1,
                f.name
            ));
        }
        for why in skipped {
            ui::hint(format!("{}: skipped {why}", ctx.paths.pretty(&path)));
        }
        if !found.is_empty() {
            sources.push(Source {
                path,
                syntax,
                found,
            });
        }
    }

    if sources.is_empty() {
        ui::info("Didn't find any aliases to import.");
        return Ok(());
    }

    ui::info("Found:");
    for s in &sources {
        ui::info(format!(
            "  {:>3} in {}",
            s.found.len(),
            ctx.paths.pretty(&s.path)
        ));
    }

    // Merge duplicates: the same alias in .bashrc and .zshrc becomes one entry.
    let mut wanted: BTreeMap<String, Alias> = BTreeMap::new();
    for s in &sources {
        for f in &s.found {
            if let Err(e) =
                safety::validate_name(&f.name).and_then(|()| safety::validate_command(&f.command))
            {
                ui::warn(format!("skipping {}: {e:#}", f.name));
                continue;
            }
            let entry = wanted.entry(f.name.clone()).or_insert_with(|| {
                let mut a = Alias::new(&f.command);
                a.shells = Vec::new();
                a
            });
            if entry.command == f.command {
                for shell in s.syntax.shells() {
                    if !entry.shells.contains(&shell) {
                        entry.shells.push(shell);
                    }
                }
            } else {
                ui::warn(format!(
                    "{} is defined differently in two files, keeping {}",
                    ui::code(&f.name),
                    ui::code(&entry.command)
                ));
            }
        }
    }
    for alias in wanted.values_mut() {
        alias.shells.sort();
        // Defined for every shell aka supports means no restriction at all.
        if alias.shells.len() == Shell::ALL.len() {
            alias.shells.clear();
        }
    }

    let mut added_total = 0;
    let changed = store::mutate(ctx, |state| {
        let mut added = 0;
        for (name, alias) in &wanted {
            let mut target = name.clone();
            if let Some(existing) = state.get(name) {
                if existing.command == alias.command {
                    continue;
                }
                match prompt::resolve_conflict(ctx, state, name, &alias.command) {
                    Ok(Resolution::Replace) => {}
                    Ok(Resolution::Rename(other)) => target = other,
                    Ok(Resolution::AlreadyThere(_)) => continue,
                    Ok(Resolution::Skip) => {
                        ui::hint(format!("kept the existing {}", ui::code(name)));
                        continue;
                    }
                    Err(e) => {
                        ui::warn(format!("{e:#}"));
                        continue;
                    }
                }
            }
            if let Some(cycle) = safety::find_loop(state, &target, &alias.command) {
                ui::warn(format!(
                    "skipping {}: it would loop ({})",
                    ui::code(&target),
                    cycle.join(" → ")
                ));
                continue;
            }
            state.insert(target.clone(), alias.clone());
            added += 1;
        }
        added_total = added;
        if added == 0 {
            return Ok(None);
        }
        Ok(Some(format!(
            "import {added} alias{}",
            if added == 1 { "" } else { "es" }
        )))
    })?;

    if ctx.dry_run {
        return Ok(());
    }
    if changed {
        ui::ok(format!(
            "Imported {added_total} alias{}.",
            if added_total == 1 { "" } else { "es" }
        ));
    } else {
        ui::ok("Everything was already in aka.");
    }

    let state = store::load(&ctx.paths)?;
    for source in &sources {
        clean_up(ctx, source, &state, args.clean)?;
    }
    Ok(())
}

/// Deletes or comments out the lines whose aliases now live in aka.
fn clean_up(ctx: &Ctx, source: &Source, state: &State, clean: Option<Clean>) -> Result<()> {
    let shell = shell_for_file(ctx, source);
    let pretty = ctx.paths.pretty(&source.path);
    // A line can only go if aka defines the same alias, with the same command,
    // in the shell that reads this file. Otherwise removing it would lose it.
    let mut lines = Vec::new();
    for f in source.found.iter().filter(|f| f.cleanable) {
        match state.get(&f.name) {
            Some(a) if a.command == f.command && a.applies_to(shell, Os::current()) => {
                lines.push(f);
            }
            Some(a) if a.command == f.command => ui::hint(format!(
                "Kept {} in {pretty}: aka's copy is disabled or doesn't apply to {shell}.",
                ui::code(&f.name)
            )),
            _ => {}
        }
    }
    if lines.is_empty() {
        return Ok(());
    }
    let count = lines.len();
    let what = if count == 1 {
        "1 line".to_string()
    } else {
        format!("{count} lines")
    };
    let question = if count == 1 {
        format!("Remove the imported alias line from {pretty}?")
    } else {
        format!("Remove the {count} imported alias lines from {pretty}?")
    };

    let choice = match clean {
        Some(c) => c,
        None if ctx.yes => Clean::Delete,
        None => {
            match prompt::choose(
                &question,
                &[('d', "delete"), ('c', "comment out"), ('k', "keep")],
                'd',
            )? {
                'd' => Clean::Delete,
                'c' => Clean::Comment,
                _ => Clean::Keep,
            }
        }
    };
    if choice == Clean::Keep {
        return Ok(());
    }

    // Without the hook the aliases would simply vanish from this shell.
    let text = fs::read_to_string(&source.path)?;
    if !setup::has_hook(&text) && !hooked(ctx, shell) {
        ui::warn(format!(
            "aka isn't hooked into {pretty} yet, so these aliases would stop loading."
        ));
        if prompt::confirm(ctx, "Add the aka hook to it now?", true)? {
            let with_hook = setup::insert_hook(&text, &setup::block(shell, &ctx.paths));
            let backup = store::backup_profile(&ctx.paths, &source.path)?;
            setup::write_profile(&source.path, &with_hook)?;
            ui::ok(format!("Hooked aka into {pretty}"));
            ui::hint(format!("  backup: {}", ctx.paths.pretty(&backup)));
        } else {
            ui::hint(format!("Kept the lines in {pretty}."));
            return Ok(());
        }
    }

    let text = fs::read_to_string(&source.path)?;
    let backup = store::backup_profile(&ctx.paths, &source.path)?;
    let mut out = String::new();
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let matches = lines
            .iter()
            .any(|f| f.line == i && line.trim_end_matches(['\n', '\r']) == f.raw);
        if matches {
            if choice == Clean::Comment {
                out.push_str("# [aka] ");
                out.push_str(line);
            }
        } else {
            out.push_str(line);
        }
    }
    setup::write_profile(&source.path, &out)?;
    let verb = if choice == Clean::Delete {
        "Removed"
    } else {
        "Commented out"
    };
    ui::ok(format!("{verb} {what} in {pretty}"));
    ui::hint(format!("  backup: {}", ctx.paths.pretty(&backup)));
    Ok(())
}

/// The shell that reads this file: zsh for .zshrc, bash for .bashrc, and so on.
fn shell_for_file(ctx: &Ctx, source: &Source) -> Shell {
    Shell::ALL
        .into_iter()
        .find(|&shell| setup::import_profiles(shell, &ctx.paths).contains(&source.path))
        .unwrap_or(source.syntax.shells()[0])
}

/// Whether the shell has the hook in one of its main profiles.
fn hooked(ctx: &Ctx, shell: Shell) -> bool {
    setup::hook_profiles(shell, &ctx.paths)
        .iter()
        .any(|p| fs::read_to_string(p).is_ok_and(|t| setup::has_hook(&t)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str, syntax: Syntax) -> Vec<(String, String, bool)> {
        parse(text, syntax)
            .0
            .into_iter()
            .map(|f| (f.name, f.command, f.cleanable))
            .collect()
    }

    fn t(n: &str, c: &str, clean: bool) -> (String, String, bool) {
        (n.into(), c.into(), clean)
    }

    #[test]
    fn posix_aliases() {
        let text = r#"
export PATH=/x:$PATH
alias gs='git status'
alias ll="ls -la"   # long listing
alias say='echo it'\''s'
alias a=b c='d e'
  alias inner='echo nested'
alias one='x'; echo hi
alias ll
# alias commented='nope'
alias -g G='| grep'
"#;
        assert_eq!(
            names(text, Syntax::Posix),
            vec![
                t("gs", "git status", true),
                t("ll", "ls -la", true),
                t("say", "echo it's", true),
                t("a", "b", false),
                t("c", "d e", false),
                t("inner", "echo nested", false),
                t("one", "x", false),
            ]
        );
        assert_eq!(
            parse(text, Syntax::Posix).1.len(),
            1,
            "the -g alias is reported as skipped"
        );
    }

    #[test]
    fn skips_the_aka_block() {
        let text = "alias a='1'\n# >>> aka >>>\nalias b='2'\n# <<< aka <<<\nalias c='3'\n";
        let got: Vec<String> = parse(text, Syntax::Posix)
            .0
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(got, ["a", "c"]);
    }

    #[test]
    fn fish_aliases() {
        let text = "alias gs 'git status'\nalias ll=\"ls -la\"\nabbr -a gco git checkout\nabbr --erase x\nalias l ls\n";
        assert_eq!(
            names(text, Syntax::Fish),
            vec![
                t("gs", "git status", true),
                t("ll", "ls -la", true),
                t("gco", "git checkout", true),
                t("l", "ls", true),
            ]
        );
    }

    #[test]
    fn powershell_aliases() {
        let text = "Set-Alias -Name g -Value git\nset-alias np notepad.exe\nNew-Alias -Name 'll' -Value 'Get-ChildItem' -Option AllScope\n";
        assert_eq!(
            names(text, Syntax::Powershell),
            vec![
                t("g", "git", true),
                t("np", "notepad.exe", true),
                t("ll", "Get-ChildItem", true)
            ]
        );
    }
}
