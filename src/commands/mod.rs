pub mod doctor;
pub mod list;
pub mod manage;

use anyhow::Result;

use crate::cli::{AddArgs, AddOpts, Cli, Command, ConfigAction, Format, ListArgs};
use crate::context::Ctx;
use crate::model::{Alias, State};
use crate::paths::Paths;
use crate::{config, import, prompt, setup, shells, store, suggest, ui};

pub fn run(cli: Cli) -> Result<()> {
    let ctx = Ctx {
        paths: Paths::resolve()?,
        yes: cli.global.yes,
        force: cli.global.force,
        dry_run: cli.global.dry_run,
    };

    // `init` only prints. `edit` and `doctor` must still run when aliases.toml
    // is broken, since they're how you find and fix the problem.
    let writes_files = !matches!(
        cli.command,
        Some(Command::Init { .. } | Command::Edit { name: None } | Command::Doctor)
    );
    if writes_files && !ctx.dry_run {
        shells::ensure_fresh(&ctx.paths)?;
    }

    match cli.command {
        None => default_view(&ctx),
        Some(command) => dispatch(&ctx, command),
    }
}

fn default_view(ctx: &Ctx) -> Result<()> {
    list::list(
        ctx,
        ListArgs {
            filter: None,
            tag: None,
            format: Format::Table,
            json: false,
            plain: false,
            names: false,
        },
    )?;
    ui::hint("Run `aka --help` to see every command.");
    Ok(())
}

fn suggest_cmd(ctx: &Ctx, limit: usize, min_count: usize) -> Result<()> {
    let files = suggest::history_files(&ctx.paths);
    if files.is_empty() {
        ui::info("Couldn't find any shell history to learn from.");
        return Ok(());
    }
    let history: Vec<String> = files.iter().flat_map(suggest::read_history).collect();
    let state = store::load(&ctx.paths)?;
    let found = suggest::suggest(&history, &state, &suggest::Options { min_count, limit });
    let sources: Vec<String> = files
        .iter()
        .map(|f| format!("{} ({})", f.shell, ctx.paths.pretty(&f.path)))
        .collect();
    ui::hint(format!(
        "Read {} commands from {}. Nothing leaves this machine.",
        history.len(),
        sources.join(", ")
    ));
    if found.is_empty() {
        ui::info("Nothing stands out yet. Try again after using your shell a bit more.");
        return Ok(());
    }

    let mut table = comfy_table::Table::new();
    table
        .load_style(comfy_table::presets::NOTHING)
        .set_header(["#", "USED", "COMMAND", "NAME"]);
    for (i, s) in found.iter().enumerate() {
        table.add_row([
            (i + 1).to_string(),
            s.count.to_string(),
            s.command.clone(),
            s.name.clone(),
        ]);
    }
    ui::print(table.to_string());

    let Some(answer) = prompt::ask("Add which? (`1 3`, `2=name`, `all`, Enter to skip)")? else {
        return Ok(());
    };
    let picks = parse_picks(&answer, found.len())?;
    for (index, name) in picks {
        let s = &found[index];
        let args = AddArgs {
            name: name.unwrap_or_else(|| s.name.clone()),
            command: vec![s.command.clone()],
            opts: AddOpts::default(),
        };
        match manage::add(ctx, args) {
            Ok(()) => {}
            Err(e) if e.is::<prompt::Cancelled>() => ui::hint(format!("Skipped {}.", s.command)),
            Err(e) => ui::error(format!("{e:#}")),
        }
    }
    Ok(())
}

/// Reads `1 3`, `2=dcu` or `all` into (index, custom name) pairs.
fn parse_picks(answer: &str, count: usize) -> Result<Vec<(usize, Option<String>)>> {
    if answer.trim().eq_ignore_ascii_case("all") {
        return Ok((0..count).map(|i| (i, None)).collect());
    }
    let mut picks = Vec::new();
    for part in answer.split([' ', ',']).filter(|p| !p.is_empty()) {
        let (number, name) = match part.split_once('=') {
            Some((n, name)) => (n, Some(name.to_string())),
            None => (part, None),
        };
        let index: usize = number
            .parse()
            .ok()
            .filter(|n| (1..=count).contains(n))
            .ok_or_else(|| anyhow::anyhow!("`{number}` isn't one of the numbers above"))?;
        picks.push((index - 1, name));
    }
    Ok(picks)
}

fn config_cmd(ctx: &Ctx, action: Option<ConfigAction>) -> Result<()> {
    let mut cfg = config::load(&ctx.paths)?;
    let (key, value) = match action {
        None => {
            for s in config::SETTINGS {
                let note = if s.is_default(&cfg) { " (default)" } else { "" };
                ui::print(format!("{} = {}{note}", s.key, s.get(&cfg)));
                ui::hint(format!("  {}", s.about));
            }
            return Ok(());
        }
        Some(ConfigAction::Get { key }) => {
            ui::print(config::find(&key)?.get(&cfg));
            return Ok(());
        }
        Some(ConfigAction::Set { key, value }) => {
            config::validate(&key, &value)?;
            (key, Some(value))
        }
        Some(ConfigAction::Unset { key }) => (key, None),
    };
    let setting = config::find(&key)?;
    let before = setting.get(&cfg);
    setting.set(&mut cfg, value.as_deref())?;
    let after = setting.get(&cfg);
    if before == after {
        ui::ok(format!("{key} is already {after}."));
        return Ok(());
    }
    if ctx.dry_run {
        ui::hint(format!(
            "Dry run: would set {key} to {after}. Nothing was saved."
        ));
        return Ok(());
    }
    config::save(&ctx.paths, &cfg)?;
    ui::ok(format!("{key} is now {after}."));
    if key == "zsh.compinit" {
        setup::refresh_zsh_block(ctx)?;
    }
    Ok(())
}

fn dispatch(ctx: &Ctx, command: Command) -> Result<()> {
    match command {
        Command::Add(args) => manage::add(ctx, args),
        Command::Rm { names, purge } => manage::rm(ctx, names, purge),
        Command::Restore { name } => manage::restore(ctx, name),
        Command::List(args) => list::list(ctx, args),
        Command::Show { name } => list::show(ctx, &name),
        Command::Rename { old, new } => manage::rename(ctx, &old, &new),
        Command::Cp { source, target } => manage::cp(ctx, &source, &target),
        Command::Edit { name } => manage::edit(ctx, name),
        Command::Enable { names, tag } => {
            let names = manage::with_tag(ctx, names, tag)?;
            manage::set_flag(ctx, names, "enable", "enabled", |a: &mut Alias| {
                !std::mem::replace(&mut a.enabled, true)
            })
        }
        Command::Disable { names, tag } => {
            let names = manage::with_tag(ctx, names, tag)?;
            manage::set_flag(ctx, names, "disable", "disabled", |a: &mut Alias| {
                std::mem::replace(&mut a.enabled, false)
            })
        }
        Command::Tag { name, tags } => manage::tag(ctx, &name, tags, true),
        Command::Untag { name, tags } => manage::tag(ctx, &name, tags, false),
        Command::Tags => list::tags(ctx),
        Command::Suggest { limit, min_count } => suggest_cmd(ctx, limit, min_count),
        Command::Lock { names } => {
            manage::set_flag(ctx, names, "lock", "locked", |a: &mut Alias| {
                !std::mem::replace(&mut a.locked, true)
            })
        }
        Command::Unlock { names } => {
            manage::set_flag(ctx, names, "unlock", "unlocked", |a: &mut Alias| {
                std::mem::replace(&mut a.locked, false)
            })
        }
        Command::Undo => {
            let message = store::undo(ctx)?;
            if !ctx.dry_run {
                ui::ok(format!("Undid \"{message}\"."));
            }
            Ok(())
        }
        Command::History { limit } => list::history(ctx, limit),
        Command::Setup {
            shells,
            no_completion,
        } => setup::setup(ctx, shells, no_completion),
        Command::Uninstall { shells, purge } => setup::uninstall(ctx, shells, purge),
        Command::Init { shell } => {
            let state: State = store::load(&ctx.paths)?;
            let path = ctx.paths.init_file(shell);
            let completer = shells::completer_path();
            let rc = shells::RenderCtx {
                init_path: &path,
                completer: &completer,
                os: crate::model::Os::current(),
            };
            ui::print(shells::render(shell, &state, &rc).trim_end());
            Ok(())
        }
        Command::Import(args) => import::run(ctx, args),
        Command::Doctor => doctor::run(ctx),
        Command::Config { action } => config_cmd(ctx, action),
    }
}
