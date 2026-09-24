//! `aka doctor`: checks the install and the aliases, and fixes what it safely can.

use std::fs;

use anyhow::{Result, bail};
use owo_colors::OwoColorize;

use crate::context::Ctx;
use crate::model::{Os, Shell};
use crate::{safety, setup, shells, store, ui};

#[derive(Default)]
struct Report {
    problems: usize,
    warnings: usize,
}

impl Report {
    fn ok(&self, msg: impl AsRef<str>) {
        line("ok", msg.as_ref(), |s| s.green().to_string());
    }

    fn fixed(&self, msg: impl AsRef<str>) {
        line("fixed", msg.as_ref(), |s| s.green().to_string());
    }

    fn warn(&mut self, msg: impl AsRef<str>) {
        self.warnings += 1;
        line("warn", msg.as_ref(), |s| s.yellow().to_string());
    }

    fn fail(&mut self, msg: impl AsRef<str>) {
        self.problems += 1;
        line("fail", msg.as_ref(), |s| s.red().to_string());
    }

    fn info(&self, msg: impl AsRef<str>) {
        line("info", msg.as_ref(), |s| s.dimmed().to_string());
    }
}

fn line(tag: &str, msg: &str, paint: impl Fn(&str) -> String) {
    let tag = format!("{tag:>5}");
    let tag = if ui::color_stdout() { paint(&tag) } else { tag };
    ui::print(format!("{tag}  {msg}"));
}

pub fn run(ctx: &Ctx) -> Result<()> {
    let mut r = Report::default();
    let paths = &ctx.paths;

    // The binary
    match which::which("aka") {
        Ok(p) => r.ok(format!(
            "aka {} at {}",
            env!("CARGO_PKG_VERSION"),
            paths.pretty(&p)
        )),
        Err(_) => r.warn("aka isn't on your PATH, so the shell hook can't call it"),
    }

    // The data
    let state = match store::load(paths) {
        Ok(s) => {
            r.ok(format!(
                "{} alias{} in {}",
                s.aliases.aliases.len(),
                if s.aliases.aliases.len() == 1 {
                    ""
                } else {
                    "es"
                },
                paths.pretty(&paths.aliases_file())
            ));
            s
        }
        Err(e) => {
            r.fail(format!("{e:#}"));
            bail!("fix the alias file first, then run `aka doctor` again");
        }
    };

    // Generated init files
    if shells::init_files_current(paths, &state) {
        r.ok("init files are up to date");
    } else if ctx.dry_run {
        r.warn("init files are out of date (run without --dry-run to fix)");
    } else {
        store::regenerate(paths)?;
        r.fixed("regenerated the init files");
    }

    // Shell hooks
    let mut any_hook = false;
    for shell in Shell::ALL {
        let profiles = setup::hook_profiles(shell, paths);
        let hooked: Vec<_> = profiles
            .iter()
            .filter(|p| fs::read_to_string(p).is_ok_and(|t| setup::has_hook(&t)))
            .collect();
        if !hooked.is_empty() {
            any_hook = true;
            let files = hooked
                .iter()
                .map(|p| paths.pretty(p))
                .collect::<Vec<_>>()
                .join(", ");
            r.ok(format!("{shell}: hooked in {files}"));
            if shell == Shell::Zsh {
                match setup::zsh_completion(paths) {
                    setup::ZshCompletion::On => r.ok("zsh: tab completion is on"),
                    setup::ZshCompletion::OnByAka => {
                        r.ok("zsh: tab completion is on (turned on by aka)")
                    }
                    setup::ZshCompletion::Off => r.warn(
                        "zsh: tab completion is off. Run `aka setup --shell zsh` to turn it on",
                    ),
                }
            }
        } else if setup::installed(shell) {
            r.warn(format!(
                "{shell}: installed but not hooked in (run `aka setup --shell {shell}`)"
            ));
        }
    }
    if !any_hook {
        r.fail("no shell loads your aliases yet. Run `aka setup`");
    }

    // The aliases themselves
    let os = Os::current();
    for (name, alias) in &state.aliases.aliases {
        if let Some(cycle) = safety::find_loop(&state, name, &alias.command) {
            r.fail(format!(
                "{name}: aliases call each other in a loop ({})",
                cycle.join(" → ")
            ));
        }
        if !alias.enabled || !(alias.os.is_empty() || alias.os.contains(&os)) {
            continue;
        }
        if let Some(program) = missing_program(&state, name, &alias.command) {
            r.warn(format!(
                "{name}: `{program}` isn't installed or isn't on your PATH"
            ));
        }
        if let Some(shadow) = safety::shadows(name)
            && !safety::is_wrapper(name, &alias.command)
        {
            r.info(format!("{name}: hides {shadow}"));
        }
        let danger = safety::danger(&alias.command);
        if !danger.is_empty() && !alias.confirm {
            r.info(format!(
                "{name}: {} (consider `aka edit` and --confirm)",
                danger.join(", ")
            ));
        }
    }

    let trashed = state.trash.len();
    if trashed > 0 {
        r.info(format!(
            "{trashed} alias{} in the trash",
            if trashed == 1 { "" } else { "es" }
        ));
    }
    r.info(format!(
        "{} backup{} kept for undo",
        store::snapshot_count(paths),
        if store::snapshot_count(paths) == 1 {
            ""
        } else {
            "s"
        }
    ));

    ui::print("");
    if r.problems > 0 {
        bail!(
            "found {} problem{}",
            r.problems,
            if r.problems == 1 { "" } else { "s" }
        );
    }
    if r.warnings > 0 {
        ui::info(format!(
            "All good, with {} warning{}.",
            r.warnings,
            if r.warnings == 1 { "" } else { "s" }
        ));
    } else {
        ui::ok("Everything looks good.");
    }
    Ok(())
}

/// The first program an alias calls, if it can't be found. Skips words that
/// aren't plain program names (variables, paths with expansions, other aliases, builtins).
fn missing_program(state: &crate::model::State, name: &str, command: &str) -> Option<String> {
    let program = safety::first_words(command).into_iter().next()?;
    let plain = program
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./+".contains(c));
    if !plain
        || program == name
        || state.contains(&program)
        || matches!(safety::shadows(&program), Some(safety::Shadow::Builtin))
    {
        return None;
    }
    if which::which(&program).is_ok() {
        return None;
    }
    // PowerShell cmdlets like Get-ChildItem aren't files on PATH.
    if program.contains('-')
        && program
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
    {
        return None;
    }
    Some(program)
}
