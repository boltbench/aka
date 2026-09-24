//! Commands that change aliases: add, rm, restore, rename, cp, edit and the on/off switches.

use anyhow::{Context, Result, anyhow, bail};

use crate::cli::AddArgs;
use crate::context::Ctx;
use crate::model::{Alias, AliasFile, Shell, State, now};
use crate::prompt::{self, Resolution};
use crate::{safety, setup, store, ui};

pub fn add(ctx: &Ctx, mut args: AddArgs) -> Result<()> {
    let trailing = args
        .split_trailing_options()
        .map_err(|e| anyhow!("{}", e.to_string().trim_start_matches("error: ").trim()))?;
    let merged;
    let ctx = match trailing {
        Some(g) => {
            merged = Ctx {
                yes: ctx.yes || g.yes,
                force: ctx.force || g.force,
                dry_run: ctx.dry_run || g.dry_run,
                ..ctx.clone()
            };
            &merged
        }
        None => ctx,
    };
    let misplaced = args.misplaced_options();
    let command = join_command(&args.command);
    safety::validate_name(&args.name)?;
    safety::validate_command(&command)?;

    let mut alias = Alias::new(&command);
    let opts = args.opts;
    for tag in &opts.tags {
        safety::validate_tag(tag)?;
    }
    alias.description = opts.description.filter(|d| !d.trim().is_empty());
    alias.tags = sorted_tags(opts.tags);
    alias.shells = dedup(opts.shells);
    alias.os = dedup(opts.os);
    alias.locked = opts.lock;
    alias.confirm = opts.confirm;

    let mut saved_as = args.name.clone();
    let mut verb = "Added";
    let changed = store::mutate(ctx, |state| {
        let mut name = args.name.clone();
        if let Some(existing) = state.get(&name) {
            if existing.command == command {
                // Same command again: treat it as an update of the other settings.
                let merged = merge(existing, &alias);
                if &merged == existing {
                    ui::ok(already_runs(&name, &command));
                    return Ok(None);
                }
                verb = "Updated";
                state.insert(name.clone(), merged);
                saved_as = name.clone();
                return Ok(Some(format!("update {name}")));
            }
            match prompt::resolve_conflict(ctx, state, &name, &command)? {
                Resolution::Replace => verb = "Replaced",
                Resolution::Rename(other) => {
                    if state.contains(&other) {
                        verb = "Replaced";
                    }
                    name = other;
                }
                Resolution::AlreadyThere(other) => {
                    ui::ok(already_runs(&other, &command));
                    return Ok(None);
                }
                Resolution::Skip => return Err(prompt::cancelled()),
            }
        }
        check_safety(ctx, state, &name, &alias, true)?;
        let message = format!(
            "{} {name}",
            if verb == "Replaced" { "replace" } else { "add" }
        );
        state.insert(name.clone(), alias.clone());
        saved_as = name;
        Ok(Some(message))
    })?;

    if changed {
        ui::ok(format!("{verb} {} → {}", ui::code(&saved_as), command));
        syntax_hint(&alias, &saved_as);
        if let Some(count) = misplaced {
            let split = args.command.len() - count;
            let options = join_command(&args.command[split..]);
            let wanted = join_command(&args.command[..split]);
            ui::warn(format!(
                "{} became part of the command. If it was meant for aka, run:",
                ui::code(&options)
            ));
            ui::hint(format!(
                "  aka add -f {options} {saved_as} {}",
                crate::shells::sh_quote(&wanted)
            ));
        }
        setup_hint(ctx);
    }
    Ok(())
}

/// Points out shells the command probably won't work in, with a fix.
fn syntax_hint(alias: &Alias, name: &str) {
    let mut broken: Vec<(Shell, &str)> = safety::syntax_issues(&alias.command)
        .into_iter()
        .filter(|(shell, _)| alias.shells.is_empty() || alias.shells.contains(shell))
        .collect();
    if broken.is_empty() {
        return;
    }
    broken.sort();
    let names: Vec<&str> = broken.iter().map(|(s, _)| s.name()).collect();
    let keep: Vec<&str> = Shell::ALL
        .iter()
        .filter(|s| !names.contains(&s.name()))
        .filter(|s| alias.shells.is_empty() || alias.shells.contains(s))
        .map(|s| s.name())
        .collect();
    ui::warn(format!(
        "{} {}, so it probably won't work in {}.",
        ui::code(&alias.command),
        broken[0].1,
        join_or(&names)
    ));
    if !keep.is_empty() {
        ui::hint(format!(
            "  To only use it where it works: aka add -f --shell {} {name} {}",
            keep.join(","),
            crate::shells::sh_quote(&alias.command)
        ));
    }
}

/// "a", "a or b", "a, b or c"
fn join_or(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    }
}

fn already_runs(name: &str, command: &str) -> String {
    format!(
        "{} already runs {}, nothing to change.",
        ui::code(name),
        ui::code(command)
    )
}

/// Keeps the old values for anything the new `add` didn't specify.
fn merge(old: &Alias, new: &Alias) -> Alias {
    Alias {
        command: new.command.clone(),
        description: new.description.clone().or_else(|| old.description.clone()),
        tags: sorted_tags(old.tags.iter().chain(&new.tags).cloned().collect()),
        enabled: old.enabled,
        shells: if new.shells.is_empty() {
            old.shells.clone()
        } else {
            new.shells.clone()
        },
        os: if new.os.is_empty() {
            old.os.clone()
        } else {
            new.os.clone()
        },
        locked: old.locked || new.locked,
        confirm: old.confirm || new.confirm,
        created_at: old.created_at.clone(),
    }
}

/// Loop, shadowing and danger checks. Loops are always an error; the other two
/// ask for confirmation unless `--force` or `--yes` was given.
pub fn check_safety(
    ctx: &Ctx,
    state: &State,
    name: &str,
    alias: &Alias,
    check_danger: bool,
) -> Result<()> {
    if let Some(cycle) = safety::find_loop(state, name, &alias.command) {
        bail!(
            "{} would make aliases call each other in a loop: {}",
            ui::code(name),
            cycle.join(" → ")
        );
    }
    if ctx.force {
        return Ok(());
    }
    if let Some(shadow) = safety::shadows(name)
        && !safety::is_wrapper(name, &alias.command)
    {
        ui::warn(format!(
            "{} will hide {shadow}. You can still reach it with {}.",
            ui::code(name),
            ui::code(format!("command {name}"))
        ));
        if !prompt::confirm(ctx, "Use this name anyway?", false)? {
            return Err(prompt::cancelled());
        }
    }
    if check_danger && !alias.confirm {
        let reasons = safety::danger(&alias.command);
        if !reasons.is_empty() {
            ui::warn(format!(
                "{} {}.",
                ui::code(&alias.command),
                reasons.join(", ")
            ));
            ui::hint("Tip: add --confirm and the alias will ask before each run.");
            if !prompt::confirm(ctx, "Save it anyway?", false)? {
                return Err(prompt::cancelled());
            }
        }
    }
    Ok(())
}

pub fn rm(ctx: &Ctx, names: Vec<String>, purge: bool) -> Result<()> {
    let names = dedup(names);
    let changed = store::mutate(ctx, |state| {
        for name in &names {
            existing(state, name)?;
        }
        for name in &names {
            let alias = state.remove(name).expect("checked above");
            // --purge only skips the trash for this alias. Older trashed
            // versions with the same name stay where they are.
            if !purge {
                state.trash.push(name, alias);
            }
        }
        Ok(Some(format!(
            "{} {}",
            if purge { "delete" } else { "remove" },
            names.join(", ")
        )))
    })?;
    if changed {
        let list = names.iter().map(ui::code).collect::<Vec<_>>().join(", ");
        if purge {
            ui::ok(format!("Deleted {list} for good."));
        } else {
            ui::ok(format!("Removed {list}."));
            ui::hint(format!(
                "Changed your mind? Run `aka restore {}`.",
                names[0]
            ));
        }
    }
    Ok(())
}

pub fn restore(ctx: &Ctx, name: Option<String>) -> Result<()> {
    let Some(name) = name else {
        return super::list::trash(ctx);
    };
    let mut saved_as = name.clone();
    let changed = store::mutate(ctx, |state| {
        let Some(trashed) = state.trash.pop(&name) else {
            bail!(
                "{} isn't in the trash. See what is with `aka restore`.",
                ui::code(&name)
            );
        };
        let mut target = name.clone();
        if state.contains(&target) {
            match prompt::resolve_conflict(ctx, state, &target, &trashed.alias.command)? {
                Resolution::Replace => {}
                Resolution::Rename(other) => target = other,
                Resolution::AlreadyThere(other) => {
                    ui::ok(already_runs(&other, &trashed.alias.command));
                    return Ok(None);
                }
                Resolution::Skip => return Err(prompt::cancelled()),
            }
        }
        if let Some(cycle) = safety::find_loop(state, &target, &trashed.alias.command) {
            bail!(
                "restoring {} would make aliases call each other in a loop: {}",
                ui::code(&target),
                cycle.join(" → ")
            );
        }
        state.insert(target.clone(), trashed.alias);
        saved_as = target;
        Ok(Some(format!("restore {name}")))
    })?;
    if changed {
        ui::ok(format!("Restored {}.", ui::code(&saved_as)));
    }
    Ok(())
}

/// Shared by rename and cp: puts a copy of `source` under `target`.
fn copy_to(ctx: &Ctx, source: &str, target: &str, keep_source: bool) -> Result<()> {
    safety::validate_name(target)?;
    let mut saved_as = target.to_string();
    let changed = store::mutate(ctx, |state| {
        let alias = existing(state, source)?.clone();
        if source == target {
            return Ok(None);
        }
        let mut dest = target.to_string();
        if state.contains(&dest) {
            match prompt::resolve_conflict(ctx, state, &dest, &alias.command)? {
                Resolution::Replace => {}
                Resolution::Rename(other) => dest = other,
                Resolution::AlreadyThere(other) => {
                    ui::ok(already_runs(&other, &alias.command));
                    return Ok(None);
                }
                Resolution::Skip => return Err(prompt::cancelled()),
            }
        }
        let mut probe = state.clone();
        if !keep_source {
            probe.remove(source);
        }
        check_safety(ctx, &probe, &dest, &alias, false)?;
        *state = probe;
        let mut copy = alias;
        if keep_source {
            copy.created_at = now();
        }
        state.insert(dest.clone(), copy);
        saved_as = dest.clone();
        Ok(Some(format!(
            "{} {source} to {dest}",
            if keep_source { "copy" } else { "rename" }
        )))
    })?;
    if changed {
        let verb = if keep_source { "Copied" } else { "Renamed" };
        ui::ok(format!(
            "{verb} {} to {}.",
            ui::code(source),
            ui::code(&saved_as)
        ));
    }
    Ok(())
}

pub fn rename(ctx: &Ctx, old: &str, new: &str) -> Result<()> {
    copy_to(ctx, old, new, false)
}

pub fn cp(ctx: &Ctx, source: &str, target: &str) -> Result<()> {
    copy_to(ctx, source, target, true)
}

/// enable, disable, lock and unlock all flip one field on a list of aliases.
pub fn set_flag(
    ctx: &Ctx,
    names: Vec<String>,
    verb: &str,
    past: &str,
    apply: fn(&mut Alias) -> bool,
) -> Result<()> {
    let names = dedup(names);
    let mut changed_names = Vec::new();
    store::mutate(ctx, |state| {
        for name in &names {
            existing(state, name)?;
        }
        for name in &names {
            if apply(state.get_mut(name).expect("checked above")) {
                changed_names.push(name.clone());
            } else {
                ui::hint(format!("{name} was already {past}."));
            }
        }
        if changed_names.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!("{verb} {}", changed_names.join(", "))))
    })?;
    if !changed_names.is_empty() && !ctx.dry_run {
        let list = changed_names
            .iter()
            .map(ui::code)
            .collect::<Vec<_>>()
            .join(", ");
        let mut past = past.to_string();
        if let Some(first) = past.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        ui::ok(format!("{past} {list}."));
    }
    Ok(())
}

pub fn edit(ctx: &Ctx, name: Option<String>) -> Result<()> {
    match name {
        Some(name) => edit_one(ctx, &name),
        None => edit_file(ctx),
    }
}

fn edit_one(ctx: &Ctx, name: &str) -> Result<()> {
    let changed = store::mutate(ctx, |state| {
        let old = existing(state, name)?.clone();
        // store::mutate refuses the change anyway; checking here just saves
        // the user from answering the prompts first.
        if old.locked && !ctx.force {
            bail!(
                "{} is locked. Run {} first, or pass --force.",
                ui::code(name),
                ui::code(format!("aka unlock {name}"))
            );
        }
        ui::info(format!(
            "Editing {}. Press Enter to keep the current value.",
            ui::code(name)
        ));
        let mut new = old.clone();
        if let Some(cmd) = prompt::ask(&format!("Command [{}]:", old.command))?
            && !cmd.is_empty()
        {
            safety::validate_command(&cmd)?;
            new.command = cmd;
        }
        let current = old.description.as_deref().unwrap_or("none");
        if let Some(desc) = prompt::ask(&format!("Description [{current}] (type - to clear):"))? {
            match desc.as_str() {
                "" => {}
                "-" => new.description = None,
                _ => new.description = Some(desc),
            }
        }
        if new == old {
            ui::hint("Nothing changed.");
            return Ok(None);
        }
        if new.command != old.command {
            let mut probe = state.clone();
            probe.remove(name);
            check_safety(ctx, &probe, name, &new, true)?;
        }
        state.insert(name, new);
        Ok(Some(format!("edit {name}")))
    })?;
    if changed {
        ui::ok(format!("Saved {}.", ui::code(name)));
    }
    Ok(())
}

/// Opens a copy of aliases.toml in $VISUAL/$EDITOR and saves it back once it
/// parses and every entry passes validation. Also the way to repair the file
/// when it's broken, since every other command refuses to load it.
fn edit_file(ctx: &Ctx) -> Result<()> {
    let broken = store::load(&ctx.paths).is_err();
    let changed = if broken {
        ui::warn("aliases.toml has an error. Opening it so you can fix it.");
        store::repair(ctx, |raw| edit_until_valid(ctx, raw, None))?
    } else {
        store::mutate(ctx, |state| {
            let text = toml::to_string_pretty(&state.aliases)?;
            let edited = edit_until_valid(ctx, &text, Some(&state.aliases))?;
            // Aliases deleted in the editor go to the trash like `aka rm` would.
            for (name, alias) in &state.aliases.aliases {
                if !edited.aliases.contains_key(name) {
                    state.trash.push(name, alias.clone());
                }
            }
            state.aliases = edited;
            Ok(Some("edit aliases.toml".to_string()))
        })?
    };
    if changed {
        ui::ok("Saved your aliases.");
    } else if !ctx.dry_run {
        ui::hint("No changes.");
    }
    Ok(())
}

/// Opens `text` in the editor until it comes back valid, or the user gives up.
/// With `before`, locked aliases must also come back unchanged (unless --force).
fn edit_until_valid(ctx: &Ctx, text: &str, before: Option<&AliasFile>) -> Result<AliasFile> {
    let asker = Ctx {
        yes: false,
        ..ctx.clone()
    };
    let tmp = tempfile::Builder::new()
        .prefix("aka-aliases-")
        .suffix(".toml")
        .tempfile()?;
    let header = "# Your aka aliases. Save and close the editor when you're done.\n\
                  # Each alias needs at least a `command`. See `aka add --help` for the other fields.\n\n";
    let body = if text.starts_with("# Your aka aliases") {
        text.to_string()
    } else {
        format!("{header}{text}")
    };
    std::fs::write(tmp.path(), body)?;
    loop {
        open_editor(tmp.path())?;
        let edited = std::fs::read_to_string(tmp.path())?;
        let result = parse_edited(&edited).and_then(|file| {
            if !ctx.force
                && let Some(before) = before
                && let Some(name) = store::locked_change(before, &file)
            {
                bail!(
                    "{} is locked, so it has to stay as it was. Run `aka unlock {name}` first, or pass --force",
                    ui::code(&name)
                );
            }
            Ok(file)
        });
        match result {
            Ok(file) => return Ok(file),
            Err(e) => {
                ui::error(format!("{e:#}"));
                if !prompt::confirm(
                    &asker,
                    "Open the editor again? (no throws your edits away)",
                    true,
                )? {
                    return Err(prompt::cancelled());
                }
            }
        }
    }
}

fn parse_edited(text: &str) -> Result<AliasFile> {
    let file: AliasFile = toml::from_str(text).context("that isn't valid TOML")?;
    let state = State {
        aliases: file.clone(),
        ..State::default()
    };
    for (name, alias) in &file.aliases {
        safety::validate_name(name).with_context(|| format!("problem with alias `{name}`"))?;
        safety::validate_command(&alias.command)
            .with_context(|| format!("problem with alias `{name}`"))?;
        if let Some(cycle) = safety::find_loop(&state, name, &alias.command) {
            bail!("aliases call each other in a loop: {}", cycle.join(" → "));
        }
    }
    Ok(file)
}

fn open_editor(path: &std::path::Path) -> Result<()> {
    let editor = ["VISUAL", "EDITOR"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(windows) {
                "notepad".into()
            } else {
                "vi".into()
            }
        });
    let mut parts = editor.split_whitespace();
    let program = parts.next().context("$EDITOR is empty")?;
    let status = std::process::Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .with_context(|| {
            format!("couldn't start your editor `{editor}`. Set $EDITOR to one that works")
        })?;
    if !status.success() {
        bail!("the editor exited with {status}, so nothing was saved");
    }
    Ok(())
}

/// Looks up an alias, with a helpful error when it doesn't exist.
pub fn existing<'a>(state: &'a State, name: &str) -> Result<&'a Alias> {
    state.get(name).ok_or_else(|| not_found(state, name))
}

pub fn not_found(state: &State, name: &str) -> anyhow::Error {
    if state.trash.contains(name) {
        return anyhow!(
            "{} is in the trash. Bring it back with {}.",
            ui::code(name),
            ui::code(format!("aka restore {name}"))
        );
    }
    let closest = state
        .aliases
        .aliases
        .keys()
        .map(|k| (distance(k, name), k))
        .filter(|(d, _)| *d <= 2)
        .min();
    match closest {
        Some((_, k)) => anyhow!(
            "there's no alias named {}. Did you mean {}?",
            ui::code(name),
            ui::code(k)
        ),
        None => anyhow!(
            "there's no alias named {}. See them all with `aka list`.",
            ui::code(name)
        ),
    }
}

/// Levenshtein distance, for "did you mean" suggestions.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != *cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// `aka add x echo "a b"` arrives as ["echo", "a b"]; put quotes back where the shell removed them.
fn join_command(parts: &[String]) -> String {
    if parts.len() == 1 {
        return parts[0].clone();
    }
    parts
        .iter()
        .map(|p| {
            if p.is_empty()
                || p.chars()
                    .any(|c| c.is_whitespace() || "'\"$`\\|&;<>()*?#".contains(c))
            {
                crate::shells::sh_quote(p)
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sorted_tags(mut tags: Vec<String>) -> Vec<String> {
    tags.sort();
    tags.dedup();
    tags
}

/// Adds or removes tags on one alias.
pub fn tag(ctx: &Ctx, name: &str, tags: Vec<String>, add: bool) -> Result<()> {
    for t in &tags {
        safety::validate_tag(t)?;
    }
    let changed = store::mutate(ctx, |state| {
        existing(state, name)?;
        let alias = state.get_mut(name).expect("checked above");
        let before = alias.tags.clone();
        if add {
            alias.tags = sorted_tags(before.iter().chain(&tags).cloned().collect());
        } else {
            alias.tags.retain(|t| !tags.contains(t));
        }
        if alias.tags == before {
            return Ok(None);
        }
        Ok(Some(format!(
            "{} {name} {}",
            if add { "tag" } else { "untag" },
            tags.join(", ")
        )))
    })?;
    if changed {
        let list = tags.join(", ");
        if add {
            ui::ok(format!("Tagged {} with {list}.", ui::code(name)));
        } else {
            ui::ok(format!("Removed {list} from {}.", ui::code(name)));
        }
    } else if !ctx.dry_run {
        ui::hint("Nothing to change.");
    }
    Ok(())
}

/// The names given, plus every alias with `tag` if there is one.
pub fn with_tag(ctx: &Ctx, mut names: Vec<String>, tag: Option<String>) -> Result<Vec<String>> {
    let Some(tag) = tag else {
        return Ok(names);
    };
    let state = store::load(&ctx.paths)?;
    let tagged: Vec<String> = state
        .aliases
        .aliases
        .iter()
        .filter(|(_, a)| a.tags.contains(&tag))
        .map(|(n, _)| n.clone())
        .collect();
    if tagged.is_empty() {
        bail!(
            "no aliases are tagged {}. See your tags with `aka tags`",
            ui::code(&tag)
        );
    }
    names.extend(tagged);
    Ok(names)
}

fn dedup<T: PartialEq>(items: Vec<T>) -> Vec<T> {
    let mut out = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// After the first alias is added, remind people that the shell needs the hook.
fn setup_hint(ctx: &Ctx) {
    if ctx.dry_run || setup::any_hook_installed(&ctx.paths) {
        return;
    }
    ui::hint("Run `aka setup` once so your shells load these aliases.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_commands() {
        assert_eq!(join_command(&["git status".into()]), "git status");
        assert_eq!(join_command(&["ls".into(), "-la".into()]), "ls -la");
        assert_eq!(join_command(&["echo".into(), "a b".into()]), "echo 'a b'");
    }

    #[test]
    fn edit_distance() {
        assert_eq!(distance("gs", "gz"), 1);
        assert_eq!(distance("status", "stauts"), 2);
        assert_eq!(distance("", "abc"), 3);
    }
}
