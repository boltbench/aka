//! Read-only commands: list, show, history and the trash listing.

use anyhow::Result;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table, presets};
use serde::Serialize;

use crate::cli::{Format, ListArgs};
use crate::context::Ctx;
use crate::model::{Alias, Os, Shell};
use crate::{safety, store, ui};

use super::manage::existing;

#[derive(Serialize)]
struct JsonAlias<'a> {
    name: &'a str,
    command: &'a str,
    description: Option<&'a str>,
    enabled: bool,
    shells: &'a [Shell],
    os: &'a [Os],
    locked: bool,
    confirm: bool,
    created_at: &'a str,
}

pub fn list(ctx: &Ctx, args: ListArgs) -> Result<()> {
    let state = store::load(&ctx.paths)?;
    let needle = args.filter.as_deref().map(str::to_lowercase);
    let items: Vec<(&String, &Alias)> = state
        .aliases
        .aliases
        .iter()
        .filter(|(name, alias)| match &needle {
            None => true,
            Some(n) => {
                name.to_lowercase().contains(n)
                    || alias.command.to_lowercase().contains(n)
                    || alias
                        .description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(n)
            }
        })
        .collect();

    if args.names {
        for (name, _) in &items {
            ui::print(name);
        }
        return Ok(());
    }

    let format = if args.json {
        Format::Json
    } else if args.plain {
        Format::Plain
    } else {
        args.format
    };

    match format {
        Format::Json => {
            let out: Vec<JsonAlias> = items
                .iter()
                .map(|(name, a)| JsonAlias {
                    name,
                    command: &a.command,
                    description: a.description.as_deref(),
                    enabled: a.enabled,
                    shells: &a.shells,
                    os: &a.os,
                    locked: a.locked,
                    confirm: a.confirm,
                    created_at: &a.created_at,
                })
                .collect();
            ui::print(serde_json::to_string_pretty(&out)?);
        }
        Format::Plain => {
            for (name, alias) in &items {
                ui::print(format!("{name}\t{}", alias.command));
            }
        }
        Format::Table => {
            if items.is_empty() {
                match &args.filter {
                    Some(f) => ui::info(format!("No aliases match {}.", ui::code(f))),
                    None => ui::info("No aliases yet. Add one with `aka add gs git status`."),
                }
                return Ok(());
            }
            let show_desc = items.iter().any(|(_, a)| a.description.is_some());
            let show_notes = items.iter().any(|(_, a)| !a.notes().is_empty());
            let color = ui::color_stdout();

            let mut table = Table::new();
            table
                .load_style(presets::NOTHING)
                .set_content_arrangement(arrangement());
            let mut header = vec!["NAME", "COMMAND"];
            if show_desc {
                header.push("DESCRIPTION");
            }
            if show_notes {
                header.push("NOTES");
            }
            table.set_header(
                header
                    .into_iter()
                    .map(|h| styled(Cell::new(h), color, |c| c.add_attribute(Attribute::Bold))),
            );

            for (name, alias) in &items {
                let off = !alias.enabled;
                let dim = |c: Cell| {
                    if off {
                        c.add_attribute(Attribute::Dim)
                    } else {
                        c
                    }
                };
                let mut row = vec![
                    styled(Cell::new(name), color, |c| dim(c.fg(Color::Cyan))),
                    styled(Cell::new(&alias.command), color, dim),
                ];
                if show_desc {
                    row.push(styled(
                        Cell::new(alias.description.as_deref().unwrap_or("")),
                        color,
                        dim,
                    ));
                }
                if show_notes {
                    row.push(styled(Cell::new(alias.notes().join(", ")), color, |c| {
                        c.fg(Color::Yellow)
                    }));
                }
                table.add_row(row);
            }
            ui::print(table.to_string());
            let total = state.aliases.aliases.len();
            let count = if items.len() == total {
                format!("{total} alias{}", if total == 1 { "" } else { "es" })
            } else {
                format!("{} of {total} aliases", items.len())
            };
            ui::hint(count);
        }
    }
    Ok(())
}

/// Wraps long cells to fit the terminal. Some terminals (tmux panes, CI logs,
/// `script`) report a width of zero, which would wrap every cell down to one
/// character, so wrapping only happens when there's a sensible width to fit.
fn arrangement() -> ContentArrangement {
    match Table::new().width() {
        Some(w) if w >= 40 => ContentArrangement::Dynamic,
        _ => ContentArrangement::Disabled,
    }
}

fn styled(cell: Cell, color: bool, style: impl FnOnce(Cell) -> Cell) -> Cell {
    if color { style(cell) } else { cell }
}

pub fn show(ctx: &Ctx, name: &str) -> Result<()> {
    let state = store::load(&ctx.paths)?;
    let alias = existing(&state, name)?;
    let all = |v: String| if v.is_empty() { "all".to_string() } else { v };
    let join = |items: Vec<String>| items.join(", ");

    let mut status = vec![if alias.enabled { "enabled" } else { "disabled" }.to_string()];
    if alias.locked {
        status.push("locked".into());
    }
    if alias.confirm {
        status.push("asks before running".into());
    }

    if ui::color_stdout() {
        ui::print(ui::bold(name));
    } else {
        ui::print(name);
    }
    ui::print(format!("  command      {}", alias.command));
    if let Some(d) = &alias.description {
        ui::print(format!("  description  {d}"));
    }
    ui::print(format!(
        "  shells       {}",
        all(join(alias.shells.iter().map(|s| s.to_string()).collect()))
    ));
    ui::print(format!(
        "  systems      {}",
        all(join(alias.os.iter().map(|s| s.to_string()).collect()))
    ));
    ui::print(format!("  status       {}", status.join(", ")));
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&alias.created_at) {
        ui::print(format!(
            "  added        {}",
            t.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
        ));
    }
    if let Some(shadow) = safety::shadows(name)
        && !safety::is_wrapper(name, &alias.command)
    {
        ui::print(format!("  note         hides {shadow}"));
    }
    let danger = safety::danger(&alias.command);
    if !danger.is_empty() {
        ui::print(format!("  careful      {}", danger.join(", ")));
    }
    Ok(())
}

pub fn history(ctx: &Ctx, limit: usize) -> Result<()> {
    let entries = store::history(&ctx.paths)?;
    if entries.is_empty() {
        ui::info("No changes yet.");
        return Ok(());
    }
    for entry in entries.iter().rev().take(limit) {
        let when = chrono::DateTime::parse_from_rfc3339(&entry.time)
            .map(|t| {
                t.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|_| entry.time.clone());
        ui::print(format!("{when}  {}", entry.message));
    }
    Ok(())
}

pub fn trash(ctx: &Ctx) -> Result<()> {
    let state = store::load(&ctx.paths)?;
    if state.trash.is_empty() {
        ui::info("The trash is empty.");
        return Ok(());
    }
    let mut table = Table::new();
    table
        .load_style(presets::NOTHING)
        .set_content_arrangement(arrangement())
        .set_header(["NAME", "COMMAND", "REMOVED"]);
    for (name, t) in state.trash.iter() {
        let when = chrono::DateTime::parse_from_rfc3339(&t.deleted_at)
            .map(|d| {
                d.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        table.add_row([name.as_str(), t.alias.command.as_str(), when.as_str()]);
    }
    ui::print(table.to_string());
    ui::hint("Bring one back with `aka restore <name>` (the newest version comes back first).");
    Ok(())
}
