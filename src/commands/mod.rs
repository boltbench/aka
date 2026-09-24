pub mod doctor;
pub mod list;
pub mod manage;

use anyhow::Result;

use crate::cli::{Cli, Command, Format, ListArgs};
use crate::context::Ctx;
use crate::model::{Alias, State};
use crate::paths::Paths;
use crate::{import, setup, shells, store, ui};

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
            format: Format::Table,
            json: false,
            plain: false,
            names: false,
        },
    )?;
    ui::hint("Run `aka --help` to see every command.");
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
        Command::Enable { names } => {
            manage::set_flag(ctx, names, "enable", "enabled", |a: &mut Alias| {
                !std::mem::replace(&mut a.enabled, true)
            })
        }
        Command::Disable { names } => {
            manage::set_flag(ctx, names, "disable", "disabled", |a: &mut Alias| {
                std::mem::replace(&mut a.enabled, false)
            })
        }
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
    }
}
