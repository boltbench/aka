mod cli;
mod commands;
mod context;
mod import;
mod model;
mod paths;
mod prompt;
mod safety;
mod setup;
mod shells;
mod store;
mod ui;

use clap::{CommandFactory, Parser};

fn main() {
    // When a shell asks for completions it runs `AKA_COMPLETE=<shell> aka -- <words>`.
    // This answers and exits before normal argument parsing.
    clap_complete::CompleteEnv::with_factory(cli::Cli::command)
        .var(shells::COMPLETE_VAR)
        .complete();

    let cli = cli::Cli::parse();
    if let Err(e) = commands::run(cli) {
        if e.is::<prompt::Cancelled>() {
            ui::hint(e.to_string());
        } else {
            ui::error(format!("{e:#}"));
        }
        std::process::exit(1);
    }
}
