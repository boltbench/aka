//! Line-based prompts. They read from stdin even when it isn't a terminal, so
//! scripts and tests can answer by piping text in. End of input means "use the default".

use std::io::{BufRead, Write};

use anyhow::Result;

use crate::context::Ctx;
use crate::model::{Alias, State};
use crate::safety;
use crate::ui;

/// Returned when the user backs out. `main` prints it as a note, not an error.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cancelled, nothing changed.")
    }
}

impl std::error::Error for Cancelled {}

pub fn cancelled() -> anyhow::Error {
    Cancelled.into()
}

/// Prints `question` and returns the trimmed answer, or `None` at end of input.
pub fn ask(question: &str) -> Result<Option<String>> {
    eprint!("{question} ");
    std::io::stderr().flush()?;
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        eprintln!();
        return Ok(None);
    }
    Ok(Some(line.trim().to_string()))
}

/// Yes/no question. `--yes` answers yes without asking.
pub fn confirm(ctx: &Ctx, question: &str, default: bool) -> Result<bool> {
    if ctx.yes {
        return Ok(true);
    }
    let options = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let Some(answer) = ask(&format!("{question} {options}"))? else {
            return Ok(default);
        };
        match answer.to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => ui::hint("Please answer y or n."),
        }
    }
}

/// Picks one of several single-letter options, e.g. `[r]eplace / [c]ancel`.
pub fn choose(question: &str, options: &[(char, &str)], default: char) -> Result<char> {
    let menu = options
        .iter()
        .map(|(key, label)| format!("[{key}]{}", &label[key.len_utf8()..]))
        .collect::<Vec<_>>()
        .join(" / ");
    let default_label = options
        .iter()
        .find(|(k, _)| *k == default)
        .map(|(_, l)| *l)
        .unwrap_or("");
    loop {
        let Some(answer) = ask(&format!("{question} {menu} (default: {default_label}):"))? else {
            return Ok(default);
        };
        let answer = answer.to_lowercase();
        if answer.is_empty() {
            return Ok(default);
        }
        if let Some((key, _)) = options
            .iter()
            .find(|(k, l)| answer == k.to_string() || answer == *l)
        {
            return Ok(*key);
        }
        ui::hint("Please pick one of the letters in brackets.");
    }
}

pub enum Resolution {
    Replace,
    Rename(String),
    /// The user picked a new name that already runs this exact command.
    AlreadyThere(String),
    Skip,
}

/// Decides what to do when `name` already exists. Shows both commands side by
/// side and offers replace, a new name, or cancel (the default).
pub fn resolve_conflict(
    ctx: &Ctx,
    state: &State,
    name: &str,
    new_command: &str,
) -> Result<Resolution> {
    let existing: &Alias = state.get(name).expect("conflict on a missing alias");
    if existing.locked && !ctx.force {
        anyhow::bail!(
            "{} is locked. Run {} first, or pass --force.",
            ui::code(name),
            ui::code(format!("aka unlock {name}"))
        );
    }
    if ctx.force || ctx.yes {
        return Ok(Resolution::Replace);
    }

    ui::info(format!("{} already exists:", ui::code(name)));
    ui::info(format!("  current  {}", existing.command));
    ui::info(format!("  new      {new_command}"));
    let pick = choose(
        "What should I do?",
        &[('r', "replace"), ('n', "new name"), ('c', "cancel")],
        'c',
    )?;
    match pick {
        'r' => Ok(Resolution::Replace),
        'n' => loop {
            let Some(new_name) = ask("New name:")? else {
                return Ok(Resolution::Skip);
            };
            if new_name.is_empty() {
                return Ok(Resolution::Skip);
            }
            if let Err(e) = safety::validate_name(&new_name) {
                ui::error(format!("{e:#}"));
                continue;
            }
            if state
                .get(&new_name)
                .is_some_and(|a| a.command == new_command)
            {
                return Ok(Resolution::AlreadyThere(new_name));
            }
            if state.contains(&new_name) {
                return resolve_conflict(ctx, state, &new_name, new_command).map(|r| match r {
                    Resolution::Replace => Resolution::Rename(new_name),
                    other => other,
                });
            }
            return Ok(Resolution::Rename(new_name));
        },
        _ => Ok(Resolution::Skip),
    }
}
