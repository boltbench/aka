//! Finding shell profiles and adding or removing the aka hook in them.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::context::Ctx;
use crate::model::{Os, Shell};
use crate::paths::Paths;
use crate::{prompt, shells, store, ui};

pub const START: &str = "# >>> aka >>>";
pub const END: &str = "# <<< aka <<<";

/// Where `aka setup` puts the hook for a shell.
pub fn hook_profiles(shell: Shell, paths: &Paths) -> Vec<PathBuf> {
    let home = &paths.home;
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    match shell {
        Shell::Bash => {
            let mut files = vec![home.join(".bashrc")];
            // Terminal.app starts login shells, which read .bash_profile and not .bashrc.
            let profile = home.join(".bash_profile");
            if Os::current() == Os::Macos
                && fs::read_to_string(&profile).is_ok_and(|t| !t.contains("bashrc"))
            {
                files.push(profile);
            }
            files
        }
        Shell::Zsh => {
            let dir = std::env::var_os("ZDOTDIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.clone());
            vec![dir.join(".zshrc")]
        }
        Shell::Fish => vec![config.join("fish").join("config.fish")],
        Shell::Powershell => {
            if cfg!(windows) {
                let docs = dirs::document_dir().unwrap_or_else(|| home.join("Documents"));
                vec![
                    docs.join("PowerShell")
                        .join("Microsoft.PowerShell_profile.ps1"),
                    docs.join("WindowsPowerShell")
                        .join("Microsoft.PowerShell_profile.ps1"),
                ]
            } else {
                vec![
                    config
                        .join("powershell")
                        .join("Microsoft.PowerShell_profile.ps1"),
                ]
            }
        }
    }
}

/// Every file `aka import` looks at for existing aliases.
pub fn import_profiles(shell: Shell, paths: &Paths) -> Vec<PathBuf> {
    let home = &paths.home;
    let mut files = hook_profiles(shell, paths);
    match shell {
        Shell::Bash => {
            for f in [".bash_profile", ".bash_aliases", ".profile"] {
                files.push(home.join(f));
            }
        }
        Shell::Zsh => {
            let dir = files[0]
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.clone());
            for f in [".zprofile", ".zshenv", ".zsh_aliases", ".aliases"] {
                files.push(dir.join(f));
            }
        }
        Shell::Fish | Shell::Powershell => {}
    }
    let mut seen = Vec::new();
    files.retain(|f| {
        let keep = !seen.contains(f);
        seen.push(f.clone());
        keep
    });
    files
}

pub fn installed(shell: Shell) -> bool {
    shell.binaries().iter().any(|b| which::which(b).is_ok())
}

/// The line that goes into a profile. Uses `$HOME` when it can, so a synced
/// dotfiles repo keeps working on other machines.
pub fn hook_line(shell: Shell, paths: &Paths) -> String {
    let init = paths.init_file(shell);
    let under_home = init.strip_prefix(&paths.home).ok();
    match shell {
        Shell::Bash | Shell::Zsh => {
            let expr = match under_home {
                Some(rest) => format!(
                    "\"$HOME/{}\"",
                    escape_dq(&slashes(rest), &['\\', '"', '$', '`'], '\\')
                ),
                None => shells::sh_quote(&init.display().to_string()),
            };
            format!("if [ -f {expr} ]; then . {expr}; fi")
        }
        Shell::Fish => {
            let expr = match under_home {
                Some(rest) => format!(
                    "\"$HOME/{}\"",
                    escape_dq(&slashes(rest), &['\\', '"', '$'], '\\')
                ),
                None => shells::fish_quote(&init.display().to_string()),
            };
            format!("if test -f {expr}; source {expr}; end")
        }
        Shell::Powershell => {
            let expr = match under_home {
                Some(rest) => format!(
                    "\"$HOME{}{}\"",
                    std::path::MAIN_SEPARATOR,
                    escape_dq(&rest.display().to_string(), &['`', '"', '$'], '`')
                ),
                None => shells::ps_quote(&init.display().to_string()),
            };
            format!("if (Test-Path -LiteralPath {expr}) {{ . {expr} }}")
        }
    }
}

fn slashes(p: &Path) -> String {
    p.display().to_string().replace('\\', "/")
}

fn escape_dq(s: &str, special: &[char], escape: char) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if special.contains(&c) {
            out.push(escape);
        }
        out.push(c);
    }
    out
}

pub fn block(shell: Shell, paths: &Paths) -> String {
    block_with(shell, paths, false)
}

/// The block, optionally also turning on zsh's tab completion. That part sits
/// before the hook line so aka's completions can register, and it lives inside
/// the block so `aka uninstall` takes it away again.
pub fn block_with(shell: Shell, paths: &Paths, zsh_completion: bool) -> String {
    let mut body = String::from(
        "# Loads your aliases. Added by `aka setup`, remove it with `aka uninstall`.\n",
    );
    if shell == Shell::Zsh && zsh_completion {
        body.push_str("# zsh tab completion was off, so aka turned it on.\n");
        for dir in HOMEBREW_ZSH_COMPLETIONS {
            if Path::new(dir).is_dir() {
                body.push_str(&format!("[[ -d {dir} ]] && fpath=({dir} $fpath)\n"));
            }
        }
        body.push_str(COMPINIT_LINE);
        body.push('\n');
    }
    format!("{START}\n{body}{}\n{END}\n", hook_line(shell, paths))
}

/// `-i` skips directories compaudit considers insecure instead of stopping the
/// shell at startup to ask about them.
const COMPINIT_LINE: &str = "autoload -Uz compinit && compinit -i";

/// Where Homebrew puts zsh completions for the tools it installs. macOS's own
/// zsh doesn't look there, so `brew`, `gh` and friends wouldn't complete.
const HOMEBREW_ZSH_COMPLETIONS: &[&str] = &[
    "/opt/homebrew/share/zsh/site-functions",
    "/home/linuxbrew/.linuxbrew/share/zsh/site-functions",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZshCompletion {
    /// Turned on by the user or a framework like oh-my-zsh.
    On,
    /// Turned on by aka's own block.
    OnByAka,
    Off,
}

/// Whether zsh's completion system is running. Asks a real interactive zsh,
/// since frameworks and plugin managers turn it on in many different ways, and
/// falls back to reading the profile if zsh can't be asked.
pub fn zsh_completion(paths: &Paths) -> ZshCompletion {
    let profiles = hook_profiles(Shell::Zsh, paths);
    let texts: Vec<String> = profiles
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok())
        .collect();
    if texts.iter().any(|t| {
        split_block(t).is_some_and(|(before, _)| t[before.len()..].contains(COMPINIT_LINE))
    }) {
        return ZshCompletion::OnByAka;
    }
    let on = probe_zsh_completion()
        .unwrap_or_else(|| texts.iter().any(|t| mentions_completion_setup(t)));
    if on {
        ZshCompletion::On
    } else {
        ZshCompletion::Off
    }
}

/// Runs `zsh -i` (which reads the user's .zshrc) and checks whether `compdef`
/// exists, the function compinit defines. Gives up after a few seconds, in case
/// the profile waits for input.
fn probe_zsh_completion() -> Option<bool> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new("zsh")
        .args(["-i", "-c", "print -r -- AKA_PROBE=${+functions[compdef]}"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    if out.contains("AKA_PROBE=1") {
        Some(true)
    } else if out.contains("AKA_PROBE=0") {
        Some(false)
    } else {
        None
    }
}

/// A best guess from the profile text: compinit called directly, or a
/// framework that calls it.
fn mentions_completion_setup(text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "compinit",
        "oh-my-zsh",
        "prezto",
        "zinit",
        "antidote",
        "antigen",
        "zplug",
        "zim",
        "sheldon",
        "znap",
    ];
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .any(|l| MARKERS.iter().any(|m| l.contains(m)))
}

/// Decides whether the zsh block should turn completion on: keep it if aka
/// already did, leave it alone if something else did, otherwise ask.
fn want_zsh_completion(ctx: &Ctx, skip: bool) -> Result<bool> {
    if skip {
        return Ok(false);
    }
    match zsh_completion(&ctx.paths) {
        ZshCompletion::OnByAka => Ok(true),
        ZshCompletion::On => Ok(false),
        ZshCompletion::Off if ctx.dry_run => {
            ui::info("zsh tab completion is off, so aka would offer to turn it on.");
            Ok(true)
        }
        ZshCompletion::Off => {
            ui::info(
                "zsh tab completion is off, so Tab won't complete aka commands, alias names,\n\
                 or anything else. aka can turn it on inside its own block (`aka uninstall` removes it).",
            );
            prompt::confirm(ctx, "Turn on zsh tab completion?", true)
        }
    }
}

pub fn has_hook(text: &str) -> bool {
    text.contains(START) && text.contains(END)
}

/// Adds the block, or replaces an existing one in place.
pub fn insert_hook(text: &str, block: &str) -> String {
    let crlf = text.contains("\r\n");
    let block = if crlf {
        block.replace('\n', "\r\n")
    } else {
        block.to_string()
    };
    if let Some((before, after)) = split_block(text) {
        return format!("{before}{block}{after}");
    }
    let nl = if crlf { "\r\n" } else { "\n" };
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push_str(nl);
        }
        out.push_str(nl);
    }
    out.push_str(&block);
    out
}

/// Takes the block out. Returns `None` when there wasn't one.
pub fn remove_hook(text: &str) -> Option<String> {
    let (before, after) = split_block(text)?;
    let before = before.trim_end_matches(['\n', '\r']);
    let after = after.trim_start_matches(['\n', '\r']);
    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    Some(match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_string(),
        (false, true) => format!("{before}{nl}"),
        (false, false) => format!("{before}{nl}{nl}{after}"),
    })
}

/// Text before the start marker's line and after the end marker's line.
fn split_block(text: &str) -> Option<(&str, &str)> {
    let start = text.find(START)?;
    let start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let end_marker = start + text[start..].find(END)?;
    let end = text[end_marker..]
        .find('\n')
        .map_or(text.len(), |i| end_marker + i + 1);
    Some((&text[..start], &text[end..]))
}

pub fn any_hook_installed(paths: &Paths) -> bool {
    Shell::ALL.iter().any(|&shell| {
        hook_profiles(shell, paths)
            .iter()
            .any(|p| fs::read_to_string(p).is_ok_and(|t| has_hook(&t)))
    })
}

/// Writes a profile. `write_atomic` follows symlinks, so dotfile managers
/// (stow, chezmoi, yadm) keep their link instead of getting a plain file.
pub fn write_profile(path: &Path, text: &str) -> Result<()> {
    store::write_atomic(path, text)
}

pub fn setup(ctx: &Ctx, shells: Vec<Shell>, no_completion: bool) -> Result<()> {
    let chosen: Vec<Shell> = if shells.is_empty() {
        Shell::ALL.into_iter().filter(|&s| installed(s)).collect()
    } else {
        shells
    };
    if chosen.is_empty() {
        bail!("couldn't find bash, zsh, fish or PowerShell. Pick one with --shell");
    }

    let zsh_completion = chosen.contains(&Shell::Zsh) && want_zsh_completion(ctx, no_completion)?;

    let mut pending = Vec::new();
    for &shell in &chosen {
        for profile in hook_profiles(shell, &ctx.paths) {
            let old = fs::read_to_string(&profile).unwrap_or_default();
            let new = insert_hook(&old, &block_with(shell, &ctx.paths, zsh_completion));
            if old == new {
                ui::ok(format!(
                    "{shell}: already set up in {}",
                    ctx.paths.pretty(&profile)
                ));
            } else {
                pending.push((shell, profile, old, new));
            }
        }
    }

    if !ctx.dry_run {
        store::regenerate(&ctx.paths)?;
    }

    if pending.is_empty() {
        ui::info("Nothing to do, aka is already hooked in.");
        return Ok(());
    }

    ui::info("aka will add a short block to:");
    for (shell, profile, old, _) in &pending {
        let note = if old.is_empty() && !profile.exists() {
            " (new file)"
        } else {
            ""
        };
        ui::info(format!(
            "  {:<11}{}{note}",
            shell.name(),
            ctx.paths.pretty(profile)
        ));
    }
    if ctx.dry_run {
        ui::hint("Dry run: nothing was changed.");
        return Ok(());
    }
    if !prompt::confirm(ctx, "Go ahead?", true)? {
        return Err(prompt::cancelled());
    }

    for (shell, profile, old, new) in &pending {
        if !old.is_empty() {
            store::backup_profile(&ctx.paths, profile)?;
        }
        write_profile(profile, new)
            .with_context(|| format!("couldn't update {}", profile.display()))?;
        ui::ok(format!(
            "{shell}: hooked into {}",
            ctx.paths.pretty(profile)
        ));
    }

    let current = current_shell()
        .filter(|s| chosen.contains(s))
        .unwrap_or(chosen[0]);
    let source = match current {
        Shell::Powershell => format!(
            ". {}",
            shells::ps_quote(&ctx.paths.init_file(current).display().to_string())
        ),
        _ => format!("source {}", ctx.paths.pretty(&ctx.paths.init_file(current))),
    };
    ui::info("");
    ui::info(format!(
        "Open a new terminal, or run this to start right away:\n  {source}"
    ));
    Ok(())
}

pub fn uninstall(ctx: &Ctx, shells: Vec<Shell>, purge: bool) -> Result<()> {
    let chosen = if shells.is_empty() {
        Shell::ALL.to_vec()
    } else {
        shells
    };
    let mut touched = 0;
    for &shell in &chosen {
        for profile in import_profiles(shell, &ctx.paths) {
            let Ok(old) = fs::read_to_string(&profile) else {
                continue;
            };
            let Some(new) = remove_hook(&old) else {
                continue;
            };
            touched += 1;
            if ctx.dry_run {
                ui::info(format!(
                    "Would remove the hook from {}",
                    ctx.paths.pretty(&profile)
                ));
                continue;
            }
            let backup = store::backup_profile(&ctx.paths, &profile)?;
            // A profile that only held the aka block (usually one `aka setup`
            // created) is removed rather than left behind empty.
            if new.trim().is_empty() && !is_symlink(&profile) {
                store::remove_if_exists(&profile)?;
            } else {
                write_profile(&profile, &new)?;
            }
            ui::ok(format!(
                "Removed the hook from {}",
                ctx.paths.pretty(&profile)
            ));
            ui::hint(format!("  backup: {}", ctx.paths.pretty(&backup)));
        }
    }
    if touched == 0 {
        ui::info("No aka hook found in your shell profiles.");
    }

    if purge {
        let root = &ctx.paths.root;
        if ctx.dry_run {
            ui::info(format!("Would delete {}", ctx.paths.pretty(root)));
        } else if root.exists() {
            if !prompt::confirm(
                ctx,
                &format!(
                    "Delete all your aliases, trash and backups in {}?",
                    ctx.paths.pretty(root)
                ),
                false,
            )? {
                return Err(prompt::cancelled());
            }
            fs::remove_dir_all(root)
                .with_context(|| format!("couldn't delete {}", root.display()))?;
            ui::ok(format!("Deleted {}", ctx.paths.pretty(root)));
        }
    } else if !ctx.dry_run && touched > 0 {
        ui::hint(format!(
            "Your aliases are still saved in {}. Run `aka setup` to hook them back in.",
            ctx.paths.pretty(&ctx.paths.root)
        ));
    }
    if touched > 0 && !ctx.dry_run {
        ui::hint("Open a new terminal for this to take effect.");
    }
    Ok(())
}

fn is_symlink(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// The shell aka is running under, based on $SHELL (or PowerShell's own variable).
pub fn current_shell() -> Option<Shell> {
    if std::env::var_os("PSModulePath").is_some() && std::env::var_os("SHELL").is_none() {
        return Some(Shell::Powershell);
    }
    let shell = std::env::var("SHELL").ok()?;
    let name = Path::new(&shell)
        .file_stem()?
        .to_string_lossy()
        .to_lowercase();
    Shell::ALL
        .into_iter()
        .find(|s| s.binaries().contains(&name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "# >>> aka >>>\nline\n# <<< aka <<<\n";

    #[test]
    fn inserts_into_empty_and_existing_files() {
        assert_eq!(insert_hook("", BLOCK), BLOCK);
        assert_eq!(
            insert_hook("export A=1", BLOCK),
            format!("export A=1\n\n{BLOCK}")
        );
        assert_eq!(
            insert_hook("export A=1\n", BLOCK),
            format!("export A=1\n\n{BLOCK}")
        );
    }

    #[test]
    fn replaces_existing_block_in_place() {
        let text = "a\n# >>> aka >>>\nold\n# <<< aka <<<\nb\n";
        assert_eq!(
            insert_hook(text, BLOCK),
            "a\n# >>> aka >>>\nline\n# <<< aka <<<\nb\n"
        );
    }

    #[test]
    fn keeps_windows_line_endings() {
        let text = "a\r\n";
        assert_eq!(
            insert_hook(text, BLOCK),
            "a\r\n\r\n# >>> aka >>>\r\nline\r\n# <<< aka <<<\r\n"
        );
    }

    #[test]
    fn removes_block() {
        let text = format!("a\n\n{BLOCK}");
        assert_eq!(remove_hook(&text).unwrap(), "a\n");
        let text = format!("a\n\n{BLOCK}\nb\n");
        assert_eq!(remove_hook(&text).unwrap(), "a\n\nb\n");
        assert_eq!(remove_hook(BLOCK).unwrap(), "");
        assert!(remove_hook("nothing here").is_none());
    }

    #[test]
    fn round_trip_leaves_file_unchanged() {
        let original = "export A=1\n";
        let with = insert_hook(original, BLOCK);
        assert_eq!(remove_hook(&with).unwrap(), original);
    }

    #[test]
    fn zsh_block_can_turn_on_completion() {
        let paths = Paths {
            root: PathBuf::from("/home/me/.config/aka"),
            home: PathBuf::from("/home/me"),
        };
        let with = block_with(Shell::Zsh, &paths, true);
        let compinit = with.find(COMPINIT_LINE).expect("compinit line");
        let hook = with.find("init.zsh").unwrap();
        assert!(
            compinit < hook,
            "completion must be on before aka registers its completions"
        );
        assert!(!block_with(Shell::Zsh, &paths, false).contains("compinit"));
        // only zsh gets it
        assert!(!block_with(Shell::Bash, &paths, true).contains("compinit"));
    }

    #[test]
    fn recognises_completion_frameworks() {
        assert!(mentions_completion_setup(
            "autoload -Uz compinit && compinit"
        ));
        assert!(mentions_completion_setup("source $ZSH/oh-my-zsh.sh"));
        assert!(!mentions_completion_setup("# compinit\nexport A=1"));
    }

    #[test]
    fn hook_lines_use_home() {
        let paths = Paths {
            root: PathBuf::from("/home/me/.config/aka"),
            home: PathBuf::from("/home/me"),
        };
        assert_eq!(
            hook_line(Shell::Zsh, &paths),
            r#"if [ -f "$HOME/.config/aka/init.zsh" ]; then . "$HOME/.config/aka/init.zsh"; fi"#
        );
        assert_eq!(
            hook_line(Shell::Fish, &paths),
            r#"if test -f "$HOME/.config/aka/init.fish"; source "$HOME/.config/aka/init.fish"; end"#
        );
        #[cfg(unix)]
        {
            let outside = Paths {
                root: PathBuf::from("/opt/aka"),
                home: PathBuf::from("/home/me"),
            };
            assert_eq!(
                hook_line(Shell::Bash, &outside),
                "if [ -f '/opt/aka/init.bash' ]; then . '/opt/aka/init.bash'; fi"
            );
        }
    }
}
