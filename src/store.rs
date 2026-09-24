//! Loading and saving aliases. Every change goes through [`mutate`], which holds
//! a file lock, snapshots the old state for `aka undo`, writes atomically, and
//! regenerates the shell init files.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::context::Ctx;
use crate::model::{Alias, AliasFile, State};
use crate::paths::Paths;
use crate::{shells, ui};

/// How many snapshots to keep for undo.
const KEEP_SNAPSHOTS: usize = 50;

pub fn load(paths: &Paths) -> Result<State> {
    Ok(State {
        aliases: read_toml(
            &paths.aliases_file(),
            "Fix it with `aka edit`, or go back with `aka undo`",
        )?,
        trash: read_toml(
            &paths.trash_file(),
            "Fix it by hand, or delete it to empty the trash",
        )?,
    })
}

fn read_toml<T: DeserializeOwned + Default>(path: &Path, fix: &str) -> Result<T> {
    match fs::read_to_string(path) {
        Ok(text) => {
            toml::from_str(&text).with_context(|| format!("{} isn't valid. {fix}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e).with_context(|| format!("couldn't read {}", path.display())),
    }
}

fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &toml::to_string_pretty(value)?)
}

/// Follows a chain of symlinks to the file that actually holds the data, even
/// when that file doesn't exist yet.
pub fn resolve_symlinks(path: &Path) -> PathBuf {
    let mut current = path.to_path_buf();
    // Bounded, in case of a symlink loop.
    for _ in 0..40 {
        match fs::read_link(&current) {
            Ok(target) if target.is_absolute() => current = target,
            Ok(target) => {
                current = current
                    .parent()
                    .map(|dir| dir.join(&target))
                    .unwrap_or(target)
            }
            Err(_) => break,
        }
    }
    current
}

/// Writes to a temp file next to `path` and renames it over the original, so a
/// crash or a full disk never leaves a half-written file behind. Symlinks are
/// followed, so a file kept in a dotfiles repo (stow, chezmoi, yadm) stays linked.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let path = &resolve_symlinks(path);
    let dir = path.parent().context("path has no parent directory")?;
    fs::create_dir_all(dir).with_context(|| format!("couldn't create {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(contents.as_bytes())?;
    tmp.as_file().sync_all()?;
    // Temp files start out private (0600); keep whatever the original had.
    if let Ok(meta) = fs::metadata(path) {
        fs::set_permissions(tmp.path(), meta.permissions())?;
    }
    tmp.persist(path)
        .with_context(|| format!("couldn't save {}", path.display()))?;
    Ok(())
}

fn save(paths: &Paths, state: &State) -> Result<()> {
    write_toml(&paths.aliases_file(), &state.aliases)?;
    if state.trash.trash.is_empty() {
        clear_file(&paths.trash_file())?;
    } else {
        write_toml(&paths.trash_file(), &state.trash)?;
    }
    Ok(())
}

/// Removes a data file, or empties it if it's a symlink so the link survives.
fn clear_file(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        write_atomic(path, "")
    } else {
        remove_if_exists(path)
    }
}

/// Runs `f` while holding the lock. Other aka processes wait their turn.
pub fn with_lock<T>(paths: &Paths, f: impl FnOnce() -> Result<T>) -> Result<T> {
    fs::create_dir_all(&paths.root)
        .with_context(|| format!("couldn't create {}", paths.root.display()))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(paths.lock_file())?;
    let mut lock = fd_lock::RwLock::new(file);
    if lock.try_write().is_err() {
        ui::hint("Waiting for another aka command to finish...");
    }
    let _guard = lock.write()?;
    f()
}

/// Applies a change. `f` edits the state and returns a short description for
/// the history log, or `None` if it decided nothing should change.
/// Returns true when something was saved.
pub fn mutate(ctx: &Ctx, f: impl FnOnce(&mut State) -> Result<Option<String>>) -> Result<bool> {
    with_lock(&ctx.paths, || {
        let before = load(&ctx.paths)?;
        let mut after = before.clone();
        let Some(message) = f(&mut after)? else {
            return Ok(false);
        };
        if after == before {
            return Ok(false);
        }
        if !ctx.force
            && let Some(name) = locked_change(&before.aliases, &after.aliases)
        {
            bail!(
                "{} is locked. Run {} first, or pass --force.",
                ui::code(&name),
                ui::code(format!("aka unlock {name}"))
            );
        }
        commit(ctx, &after, &message)
    })
}

/// Saves `state` as a new change: snapshot for undo, write, log, regenerate.
/// Must be called while holding the lock.
fn commit(ctx: &Ctx, state: &State, message: &str) -> Result<bool> {
    if ctx.dry_run {
        ui::hint(format!("Dry run: would {message}. Nothing was saved."));
        return Ok(false);
    }
    let id = snapshot(&ctx.paths)?;
    save(&ctx.paths, state)?;
    append_history(&ctx.paths, &id, message)?;
    shells::write_init_files(&ctx.paths, state)?;
    prune_snapshots(&ctx.paths)?;
    Ok(true)
}

/// The first locked alias that a change would modify or remove. Only turning
/// it on or off, or unlocking it, is allowed without --force. Every command
/// goes through this, so a lock can't be bypassed by any route.
pub fn locked_change(before: &AliasFile, after: &AliasFile) -> Option<String> {
    before
        .aliases
        .iter()
        .filter(|(_, alias)| alias.locked)
        .find(|(name, old)| match after.aliases.get(*name) {
            None => true,
            Some(new) => {
                let comparable = Alias {
                    locked: true,
                    enabled: old.enabled,
                    ..new.clone()
                };
                &comparable != *old
            }
        })
        .map(|(name, _)| name.clone())
}

/// Replaces aliases.toml when it can't be parsed. `fix` gets the broken text
/// and returns what to save instead. The broken file is kept in the snapshot,
/// so `aka undo` can bring it back.
pub fn repair(ctx: &Ctx, fix: impl FnOnce(&str) -> Result<AliasFile>) -> Result<bool> {
    with_lock(&ctx.paths, || {
        let raw = fs::read_to_string(ctx.paths.aliases_file()).unwrap_or_default();
        let trash = read_toml(
            &ctx.paths.trash_file(),
            "Fix it by hand, or delete it to empty the trash",
        )?;
        let aliases = fix(&raw)?;
        commit(ctx, &State { aliases, trash }, "repair aliases.toml")
    })
}

/// Rebuilds the init files from the data on disk. Loads while holding the lock,
/// so it never writes init files from a copy that another process just changed.
pub fn regenerate(paths: &Paths) -> Result<()> {
    with_lock(paths, || shells::write_init_files(paths, &load(paths)?))
}

/// Copies the current data files into `backups/<id>/`. Files that don't exist
/// yet are simply absent from the snapshot, and undo deletes them again.
fn snapshot(paths: &Paths) -> Result<String> {
    let id = chrono::Utc::now().format("%Y%m%dT%H%M%S%.6fZ").to_string();
    let dir = paths.backups_dir().join(&id);
    fs::create_dir_all(&dir)?;
    for file in [paths.aliases_file(), paths.trash_file()] {
        if file.exists() {
            fs::copy(&file, dir.join(file.file_name().unwrap()))?;
        }
    }
    Ok(id)
}

fn prune_snapshots(paths: &Paths) -> Result<()> {
    let mut ids = snapshot_ids(paths)?;
    if ids.len() > KEEP_SNAPSHOTS {
        ids.sort();
        for id in &ids[..ids.len() - KEEP_SNAPSHOTS] {
            fs::remove_dir_all(paths.backups_dir().join(id))?;
        }
    }
    Ok(())
}

fn snapshot_ids(paths: &Paths) -> Result<Vec<String>> {
    let Ok(entries) = fs::read_dir(paths.backups_dir()) else {
        return Ok(Vec::new());
    };
    let mut ids = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type()?.is_dir() && name != "profiles" {
            ids.push(name);
        }
    }
    Ok(ids)
}

pub fn snapshot_count(paths: &Paths) -> usize {
    snapshot_ids(paths).map(|ids| ids.len()).unwrap_or(0)
}

pub struct HistoryEntry {
    pub time: String,
    pub id: String,
    pub message: String,
}

fn append_history(paths: &Paths, id: &str, message: &str) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.history_file())?;
    writeln!(
        file,
        "{}\t{id}\t{}",
        crate::model::now(),
        message.replace(['\t', '\n'], " ")
    )?;
    Ok(())
}

pub fn history(paths: &Paths) -> Result<Vec<HistoryEntry>> {
    let text = match fs::read_to_string(paths.history_file()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some(HistoryEntry {
                time: parts.next()?.to_string(),
                id: parts.next()?.to_string(),
                message: parts.next()?.to_string(),
            })
        })
        .collect())
}

/// Restores the state from before the most recent change and drops that
/// change from the history. Returns what was undone.
pub fn undo(ctx: &Ctx) -> Result<String> {
    with_lock(&ctx.paths, || {
        let mut entries = history(&ctx.paths)?;
        let Some(last) = entries.pop() else {
            bail!("there's nothing to undo");
        };
        let dir = ctx.paths.backups_dir().join(&last.id);
        if !dir.is_dir() {
            bail!("can't undo \"{}\", its backup was pruned", last.message);
        }
        if ctx.dry_run {
            ui::hint(format!(
                "Dry run: would undo \"{}\". Nothing was changed.",
                last.message
            ));
            return Ok(last.message);
        }
        for file in [ctx.paths.aliases_file(), ctx.paths.trash_file()] {
            let saved = dir.join(file.file_name().unwrap());
            if saved.exists() {
                write_atomic(&file, &fs::read_to_string(&saved)?)?;
            } else {
                clear_file(&file)?;
            }
        }
        let rest: String = entries
            .iter()
            .map(|e| format!("{}\t{}\t{}\n", e.time, e.id, e.message))
            .collect();
        write_atomic(&ctx.paths.history_file(), &rest)?;
        fs::remove_dir_all(&dir)?;
        // Undoing a repair brings back the broken file on purpose. Keep the
        // current init files and point at the fix instead of failing.
        match load(&ctx.paths) {
            Ok(state) => shells::write_init_files(&ctx.paths, &state)?,
            Err(_) => ui::warn("the restored aliases.toml has an error. Fix it with `aka edit`."),
        }
        Ok(last.message)
    })
}

/// Copies a shell profile into `backups/profiles/<time>/` before aka edits it.
pub fn backup_profile(paths: &Paths, profile: &Path) -> Result<std::path::PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let dir = paths.profile_backups_dir().join(stamp);
    fs::create_dir_all(&dir)?;
    let name = profile
        .strip_prefix(&paths.home)
        .unwrap_or(profile)
        .to_string_lossy()
        .replace(['/', '\\', ':'], "_");
    let dest = dir.join(name.trim_start_matches('_'));
    fs::copy(profile, &dest).with_context(|| format!("couldn't back up {}", profile.display()))?;
    Ok(dest)
}

pub fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("couldn't remove {}", path.display())),
    }
}
