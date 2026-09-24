//! Terminal output. Status messages go to stderr so stdout stays clean for piping.

use std::io::IsTerminal;
use std::sync::OnceLock;

use owo_colors::OwoColorize;

fn colors_allowed() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

pub fn color_stderr() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| colors_allowed() && std::io::stderr().is_terminal())
}

pub fn color_stdout() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| colors_allowed() && std::io::stdout().is_terminal())
}

pub fn ok(msg: impl AsRef<str>) {
    let tag = if color_stderr() {
        "✓".green().bold().to_string()
    } else {
        "✓".to_string()
    };
    eprintln!("{tag} {}", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    let tag = if color_stderr() {
        "warning:".yellow().bold().to_string()
    } else {
        "warning:".to_string()
    };
    eprintln!("{tag} {}", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    let tag = if color_stderr() {
        "error:".red().bold().to_string()
    } else {
        "error:".to_string()
    };
    eprintln!("{tag} {}", msg.as_ref());
}

pub fn info(msg: impl AsRef<str>) {
    eprintln!("{}", msg.as_ref());
}

pub fn hint(msg: impl AsRef<str>) {
    if color_stderr() {
        eprintln!("{}", msg.as_ref().dimmed());
    } else {
        eprintln!("{}", msg.as_ref());
    }
}

/// Highlights a name or command inside a sentence, e.g. `gs`.
pub fn code(s: impl AsRef<str>) -> String {
    if color_stderr() {
        s.as_ref().cyan().to_string()
    } else {
        format!("`{}`", s.as_ref())
    }
}

pub fn bold(s: impl AsRef<str>) -> String {
    if color_stderr() {
        s.as_ref().bold().to_string()
    } else {
        s.as_ref().to_string()
    }
}

/// Prints a line to stdout. Exits quietly if the reader went away (`aka list | head`).
pub fn print(line: impl AsRef<str>) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    if let Err(e) = writeln!(out, "{}", line.as_ref()) {
        if e.kind() == std::io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        panic!("couldn't write to stdout: {e}");
    }
}
