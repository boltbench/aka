//! End-to-end tests. Each test gets its own fake HOME so nothing touches the real one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

struct Env {
    home: TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            home: TempDir::new().unwrap(),
        }
    }

    fn home(&self) -> &Path {
        self.home.path()
    }

    fn root(&self) -> PathBuf {
        self.home().join(".config").join("aka")
    }

    fn aka(&self) -> Command {
        let mut cmd = Command::cargo_bin("aka").unwrap();
        // Only aka's own directory on PATH, so shadow warnings don't depend on
        // what happens to be installed on the machine running the tests.
        cmd.env("PATH", bin_dir())
            .env("HOME", self.home())
            .env("AKA_HOME", self.root())
            .env("NO_COLOR", "1")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("HISTFILE")
            .env_remove("APPDATA")
            .env_remove("ZDOTDIR")
            .env_remove("VISUAL")
            .env_remove("EDITOR");
        cmd
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        self.aka().args(args).write_stdin("").assert()
    }

    fn plain_list(&self) -> String {
        let out = self.aka().args(["list", "--plain"]).output().unwrap();
        String::from_utf8(out.stdout).unwrap()
    }
}

#[test]
fn add_and_list() {
    let env = Env::new();
    env.run(&["add", "gs", "git", "status"])
        .success()
        .stderr(predicate::str::contains("Added"));
    env.run(&["add", "-d", "long listing", "ll", "ls -la"])
        .success();
    assert_eq!(env.plain_list(), "gs\tgit status\nll\tls -la\n");
    env.run(&["list"])
        .success()
        .stdout(predicate::str::contains("long listing"));
}

#[test]
fn options_after_a_quoted_command() {
    let env = Env::new();
    env.run(&["add", "gs", "git status", "-d", "quick status", "--lock"])
        .success();
    env.run(&["show", "gs"])
        .success()
        .stdout(predicate::str::contains("quick status").and(predicate::str::contains("locked")));
}

#[test]
fn conflict_defaults_to_cancel() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.aka()
        .args(["add", "gs", "git status -sb"])
        .write_stdin("\n")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("already exists").and(predicate::str::contains("Cancelled")),
        );
    assert_eq!(env.plain_list(), "gs\tgit status\n");
    // Cancelling exits with 2, so scripts can tell it apart from an error (1).
    env.aka()
        .args(["add", "gs", "other"])
        .write_stdin("c\n")
        .assert()
        .code(2);
    env.run(&["rm", "nope"]).code(1);
}

#[test]
fn conflict_replace_and_rename() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.aka()
        .args(["add", "gs", "git status -sb"])
        .write_stdin("r\n")
        .assert()
        .success();
    assert_eq!(env.plain_list(), "gs\tgit status -sb\n");

    env.aka()
        .args(["add", "gs", "git status --short"])
        .write_stdin("n\ngss\n")
        .assert()
        .success();
    assert_eq!(
        env.plain_list(),
        "gs\tgit status -sb\ngss\tgit status --short\n"
    );
}

#[test]
fn new_name_that_already_runs_the_command() {
    let env = Env::new();
    env.run(&["add", "g", "git"]).success();
    env.run(&["add", "gs", "git status"]).success();
    env.aka()
        .args(["add", "g", "git status"])
        .write_stdin("n\ngs\n")
        .assert()
        .success()
        .stderr(predicate::str::contains("already runs"));
    assert_eq!(env.plain_list(), "g\tgit\ngs\tgit status\n");
}

#[test]
fn force_replaces_without_asking() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["add", "-f", "gs", "git status -sb"]).success();
    assert_eq!(env.plain_list(), "gs\tgit status -sb\n");
}

#[test]
fn same_command_updates_other_fields() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["add", "-d", "status", "gs", "git status"])
        .success()
        .stderr(predicate::str::contains("Updated"));
    env.run(&["add", "gs", "git status"])
        .success()
        .stderr(predicate::str::contains("nothing to change"));
}

#[test]
fn rejects_bad_names_and_loops() {
    let env = Env::new();
    env.run(&["add", "has space", "x"])
        .failure()
        .stderr(predicate::str::contains("contains a space"));
    env.run(&["add", "if", "x"])
        .failure()
        .stderr(predicate::str::contains("reserved"));
    env.run(&["add", "a", "b"]).success();
    env.run(&["add", "b", "a --x"])
        .failure()
        .stderr(predicate::str::contains("loop"));
}

#[test]
fn dangerous_commands_need_confirmation() {
    let env = Env::new();
    env.run(&["add", "nuke", "rm -rf build"])
        .failure()
        .stderr(predicate::str::contains("recursively"));
    env.run(&["add", "-y", "nuke", "rm -rf build"]).success();
    // --confirm makes the alias itself ask, so no warning is needed
    env.run(&["add", "--confirm", "nuke2", "rm -rf build"])
        .success();
}

#[test]
fn locked_aliases_are_protected() {
    let env = Env::new();
    env.run(&["add", "--lock", "gs", "git status"]).success();
    env.run(&["rm", "gs"])
        .failure()
        .stderr(predicate::str::contains("locked"));
    env.run(&["add", "gs", "other"])
        .failure()
        .stderr(predicate::str::contains("locked"));
    env.run(&["rename", "gs", "g"])
        .failure()
        .stderr(predicate::str::contains("locked"));
    env.run(&["unlock", "gs"]).success();
    env.run(&["rm", "gs"]).success();
}

#[test]
fn rm_restore_and_purge() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["rm", "gs"]).success();
    assert_eq!(env.plain_list(), "");
    env.run(&["show", "gs"])
        .failure()
        .stderr(predicate::str::contains("in the trash"));
    env.run(&["restore"])
        .success()
        .stdout(predicate::str::contains("gs"));
    env.run(&["restore", "gs"]).success();
    assert_eq!(env.plain_list(), "gs\tgit status\n");
    env.run(&["rm", "--purge", "gs"]).success();
    env.run(&["restore", "gs"]).failure();
}

#[test]
fn undo_and_history() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["add", "ll", "ls -la"]).success();
    env.run(&["history"])
        .success()
        .stdout(predicate::str::contains("add ll"));
    env.run(&["undo"])
        .success()
        .stderr(predicate::str::contains("add ll"));
    assert_eq!(env.plain_list(), "gs\tgit status\n");
    env.run(&["undo"]).success();
    assert_eq!(env.plain_list(), "");
    env.run(&["undo"])
        .failure()
        .stderr(predicate::str::contains("nothing to undo"));
}

#[test]
fn dry_run_changes_nothing() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["--dry-run", "rm", "gs"])
        .success()
        .stderr(predicate::str::contains("Dry run"));
    env.run(&["add", "--dry-run", "ll", "ls"]).success();
    assert_eq!(env.plain_list(), "gs\tgit status\n");
}

#[test]
fn rename_copy_and_flags() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["cp", "gs", "gst"]).success();
    env.run(&["rename", "gs", "g"]).success();
    assert_eq!(env.plain_list(), "g\tgit status\ngst\tgit status\n");
    env.run(&["disable", "g"]).success();
    env.run(&["list"])
        .success()
        .stdout(predicate::str::contains("disabled"));
    env.run(&["rm", "nope"])
        .failure()
        .stderr(predicate::str::contains("no alias named"));
    env.run(&["rm", "gz"])
        .failure()
        .stderr(predicate::str::contains("Did you mean `g`"));
}

#[test]
fn json_output() {
    let env = Env::new();
    env.run(&["add", "--shell", "zsh,bash", "gs", "git status"])
        .success();
    let out = env.aka().args(["list", "--json"]).output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value[0]["name"], "gs");
    assert_eq!(value[0]["shells"], serde_json::json!(["zsh", "bash"]));
    assert_eq!(value[0]["enabled"], true);
}

#[test]
fn writes_init_files() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    for ext in ["bash", "zsh", "fish", "ps1"] {
        let text = fs::read_to_string(env.root().join(format!("init.{ext}"))).unwrap();
        assert!(text.contains("gs"), "init.{ext} should define gs");
    }
    env.run(&["init", "zsh"])
        .success()
        .stdout(predicate::str::contains("alias gs='git status'"));
}

#[test]
fn setup_is_idempotent_and_uninstall_restores() {
    let env = Env::new();
    let zshrc = env.home().join(".zshrc");
    fs::write(&zshrc, "export A=1\n").unwrap();
    env.run(&["setup", "-y", "--shell", "zsh"]).success();
    let once = fs::read_to_string(&zshrc).unwrap();
    assert!(once.contains("# >>> aka >>>"));
    env.run(&["setup", "-y", "--shell", "zsh"])
        .success()
        .stderr(predicate::str::contains("already set up"));
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), once);
    env.run(&["uninstall", "-y", "--shell", "zsh"]).success();
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), "export A=1\n");
}

#[test]
fn uninstall_removes_a_profile_that_setup_created() {
    let env = Env::new();
    let zshrc = env.home().join(".zshrc");
    env.run(&["setup", "-y", "--shell", "zsh"]).success();
    assert!(zshrc.exists());
    env.run(&["uninstall", "-y", "--shell", "zsh"]).success();
    assert!(!zshrc.exists());
}

#[cfg(unix)]
#[test]
fn setup_follows_symlinks() {
    let env = Env::new();
    let real = env.home().join("dotfiles-zshrc");
    fs::write(&real, "export A=1\n").unwrap();
    std::os::unix::fs::symlink(&real, env.home().join(".zshrc")).unwrap();
    env.run(&["setup", "-y", "--shell", "zsh"]).success();
    assert!(
        fs::symlink_metadata(env.home().join(".zshrc"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(fs::read_to_string(&real).unwrap().contains("# >>> aka >>>"));
}

#[test]
fn import_deletes_or_comments_lines() {
    let env = Env::new();
    let zshrc = env.home().join(".zshrc");
    fs::write(
        &zshrc,
        "export A=1\nalias gs='git status'\nalias ll=\"ls -la\" # long\nif true; then\n  alias c='echo c'\nfi\n",
    )
    .unwrap();
    env.run(&[
        "import",
        "-y",
        "--clean",
        "comment",
        "--from",
        zshrc.to_str().unwrap(),
    ])
    .success();
    assert_eq!(env.plain_list(), "c\techo c\ngs\tgit status\nll\tls -la\n");
    let text = fs::read_to_string(&zshrc).unwrap();
    assert!(text.contains("# [aka] alias gs='git status'"));
    assert!(
        text.contains("  alias c='echo c'"),
        "lines inside blocks stay untouched"
    );
    assert!(
        text.contains("# >>> aka >>>"),
        "the hook gets added so the aliases keep loading"
    );
}

#[test]
fn import_keeps_existing_on_cancel() {
    let env = Env::new();
    env.run(&["add", "gs", "git status -sb"]).success();
    let file = env.home().join("aliases.sh");
    fs::write(&file, "alias gs='git status'\n").unwrap();
    env.aka()
        .args([
            "import",
            "--clean",
            "keep",
            "--from",
            file.to_str().unwrap(),
        ])
        .write_stdin("c\n")
        .assert()
        .success();
    assert_eq!(env.plain_list(), "gs\tgit status -sb\n");
}

#[test]
fn doctor_reports_missing_hook() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["doctor"])
        .failure()
        .stdout(predicate::str::contains("no shell loads your aliases"));
}

#[test]
fn completion_lists_alias_names() {
    let env = Env::new();
    env.run(&["add", "-d", "quick status", "gs", "git status"])
        .success();
    env.aka()
        .env("AKA_COMPLETE", "fish")
        .args(["--", "aka", "rm", ""])
        .assert()
        .success()
        .stdout(predicate::str::contains("gs\tquick status"));
}

// ------------------------------------------------------------ real shells
//
// These source the generated init file in an actual shell. Each one is skipped
// when that shell isn't installed; CI installs all four.

fn bin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_aka"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Whether a real-shell test can run. bash, zsh and fish tests only run on
/// Unix (on Windows `bash` may well be WSL's). A missing shell is reported as
/// skipped, and in CI it's a failure, since CI installs every shell and a
/// silent skip there would hide a broken test.
fn has(shell: &str) -> bool {
    let unix_only = shell != "pwsh";
    if unix_only && !cfg!(unix) {
        eprintln!("skipped: {shell} tests only run on Unix");
        return false;
    }
    if which::which(shell).is_ok() {
        return true;
    }
    if std::env::var_os("CI").is_some() {
        panic!("{shell} isn't installed, but CI should have it");
    }
    eprintln!("skipped: {shell} isn't installed");
    false
}

/// Runs a script in `shell` with aka on PATH and the test's HOME.
fn in_shell(env: &Env, shell: &str, args: &[&str], script: &str) -> String {
    let path = std::env::join_paths(std::iter::once(bin_dir()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let out = StdCommand::new(shell)
        .args(args)
        .arg(script)
        .env("HOME", env.home())
        .env("AKA_HOME", env.root())
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .env("BASH_SILENCE_DEPRECATION_WARNING", "1")
        .env_remove("ZDOTDIR")
        .env_remove("XDG_CONFIG_HOME")
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn prepared() -> Env {
    let env = Env::new();
    env.run(&["add", "hello", "echo hello-from-aka"]).success();
    env.run(&["add", "--confirm", "careful", "echo ran-careful"])
        .success();
    env.run(&["add", "twice", "echo \"$1-$1\" count=$#"])
        .success();
    env
}

fn init(env: &Env, ext: &str) -> String {
    env.root().join(format!("init.{ext}")).display().to_string()
}

/// Runs the standard checks in bash or zsh: the alias works, removing it takes
/// effect in the same session, and confirm-before-run asks. `prelude` runs
/// first, to try unusual settings like `set -u`.
fn check_posix_shell(shell: &str, prelude: &str) {
    if !has(shell) {
        return;
    }
    let env = prepared();
    // zsh parses the whole -c script up front, so `eval` makes it expand
    // aliases defined along the way. bash reads a line at a time either way.
    let script = format!(
        "{prelude}\nsource '{}'\neval hello\naka rm hello >/dev/null 2>&1\neval hello 2>/dev/null || echo gone\necho y | careful\necho n | careful || echo declined\neval twice hi there",
        init(&env, shell)
    );
    // Aliases only expand in interactive shells.
    let out = in_shell(&env, shell, &["-i", "-c"], &script);
    assert!(out.contains("hello-from-aka"), "{shell} {prelude}: {out}");
    assert!(
        out.contains("gone"),
        "{shell} {prelude}: removed alias should disappear in the same session: {out}"
    );
    assert!(out.contains("ran-careful"), "{shell} {prelude}: {out}");
    assert!(out.contains("declined"), "{shell} {prelude}: {out}");
    assert!(
        out.contains("hi-hi count=2"),
        "{shell} {prelude}: arguments: {out}"
    );
    assert!(
        !out.contains("unbound variable") && !out.contains("parameter not set"),
        "{shell} {prelude}: {out}"
    );
}

#[test]
fn works_in_bash() {
    check_posix_shell("bash", "");
}

#[test]
fn works_in_bash_with_set_u() {
    check_posix_shell("bash", "set -u");
}

#[test]
fn works_in_zsh() {
    check_posix_shell("zsh", "");
}

#[test]
fn works_in_zsh_with_nounset() {
    check_posix_shell("zsh", "setopt nounset");
}

#[test]
fn works_in_fish() {
    if !has("fish") {
        return;
    }
    let env = prepared();
    let script = format!(
        "source '{}'\nhello\naka rm hello >/dev/null 2>&1\nfunctions -q hello; or echo gone\necho y | careful\necho n | careful; or echo declined\ntwice hi there",
        init(&env, "fish")
    );
    let out = in_shell(&env, "fish", &["-c"], &script);
    assert!(out.contains("hi-hi count=2"), "arguments: {out}");
    assert!(out.contains("hello-from-aka"), "{out}");
    assert!(out.contains("gone"), "{out}");
    assert!(out.contains("ran-careful"), "{out}");
    assert!(out.contains("declined"), "{out}");
}

#[test]
fn works_in_powershell() {
    // Windows always has Windows PowerShell; elsewhere it's pwsh or nothing.
    let shell = if cfg!(windows) && which::which("pwsh").is_err() {
        "powershell"
    } else if has("pwsh") {
        "pwsh"
    } else {
        return;
    };
    let env = prepared();
    // PowerShell reloads from its prompt hook, which scripts never trigger, so
    // re-source by hand to check removal.
    let init = init(&env, "ps1");
    let script = format!(
        ". '{init}'; hello; twice hi there; aka rm hello *> $null; . '{init}'; if (-not (Get-Command hello -ErrorAction SilentlyContinue)) {{ 'gone' }}"
    );
    let out = in_shell(
        &env,
        shell,
        &["-NoProfile", "-NonInteractive", "-Command"],
        &script,
    );
    assert!(out.contains("hello-from-aka"), "{out}");
    // PowerShell's echo prints each argument on its own line
    assert!(
        out.contains("hi-hi") && out.contains("count=2"),
        "arguments: {out}"
    );
    assert!(out.contains("gone"), "{out}");
}

/// `gco ma<Tab>` should complete like `git checkout ma<Tab>`, and `g` (a
/// one-word alias, so a real PowerShell alias) like `git`. A fake git
/// completer stands in for posh-git so the test doesn't depend on it.
#[test]
fn powershell_completes_through_aliases() {
    let shell = if cfg!(windows) && which::which("pwsh").is_err() {
        "powershell"
    } else if has("pwsh") {
        "pwsh"
    } else {
        return;
    };
    let env = Env::new();
    env.run(&["add", "gco", "git checkout"]).success();
    env.run(&["add", "g", "git"]).success();
    let init = init(&env, "ps1");
    let script = format!(
        ". '{init}'; \
         Register-ArgumentCompleter -Native -CommandName git -ScriptBlock {{ \
           param($w, $ast, $pos) \
           [System.Management.Automation.CompletionResult]::new('fake:' + $ast.Extent.Text, 'x', 'ParameterValue', 'x') }}; \
         (TabExpansion2 -inputScript 'gco ma' -cursorColumn 6).CompletionMatches.CompletionText; \
         (TabExpansion2 -inputScript 'g sta' -cursorColumn 5).CompletionMatches.CompletionText"
    );
    let out = in_shell(
        &env,
        shell,
        &["-NoProfile", "-NonInteractive", "-Command"],
        &script,
    );
    assert!(out.contains("fake:git checkout ma"), "{out}");
    assert!(out.contains("fake:git sta"), "{out}");
}

#[cfg(unix)]
#[test]
fn setup_moves_the_block_after_prompt_tools() {
    let env = Env::new();
    let profile = env
        .home()
        .join(".config/powershell/Microsoft.PowerShell_profile.ps1");
    env.run(&["setup", "-y", "--shell", "powershell"]).success();
    let mut text = fs::read_to_string(&profile).unwrap();
    text.push_str("Invoke-Expression (&starship init powershell)\n");
    fs::write(&profile, &text).unwrap();

    env.run(&["doctor"])
        .stdout(predicate::str::contains("starship init"));
    env.run(&["setup", "-y", "--shell", "powershell"]).success();
    let text = fs::read_to_string(&profile).unwrap();
    let starship = text.find("starship init").unwrap();
    let block = text.find("# >>> aka >>>").unwrap();
    assert!(
        block > starship,
        "aka's block should now come last:\n{text}"
    );
    assert_eq!(text.matches("# >>> aka >>>").count(), 1);
}

#[test]
fn doctor_notices_zsh_without_aliases() {
    if !has("zsh") {
        return;
    }
    let env = Env::new();
    fs::write(env.home().join(".zshrc"), "unsetopt aliases\n").unwrap();
    env.run(&["setup", "-y", "--no-completion", "--shell", "zsh"])
        .success();
    // doctor needs to find zsh to ask it, so give it the real PATH
    env.aka()
        .env("PATH", std::env::var_os("PATH").unwrap())
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("aliases are turned off"));
}

// ------------------------------------------------------------ review fixes

/// Writes a tiny editor script that replaces the file it's given with `contents`.
#[cfg(unix)]
fn fake_editor(env: &Env, contents: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let script = env.home().join("editor.sh");
    let data = env.home().join("editor-output.toml");
    fs::write(&data, contents).unwrap();
    fs::write(
        &script,
        format!("#!/bin/sh\n/bin/cat '{}' > \"$1\"\n", data.display()),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    script.display().to_string()
}

#[cfg(unix)]
#[test]
fn import_edits_a_symlinked_profile_only_once() {
    let env = Env::new();
    let profile = env.home().join(".profile");
    fs::write(
        &profile,
        "alias a='echo a'\nalias b='echo b'\nkeepme=1\nimportant=2\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&profile, env.home().join(".bash_profile")).unwrap();
    env.run(&["import", "-y", "--clean", "delete"]).success();
    let text = fs::read_to_string(&profile).unwrap();
    assert!(
        text.contains("keepme=1") && text.contains("important=2"),
        "{text}"
    );
    assert!(!text.contains("alias a="), "{text}");
}

#[cfg(unix)]
#[test]
fn symlinked_alias_file_stays_linked() {
    let env = Env::new();
    let dots = env.home().join("dots");
    fs::create_dir_all(&dots).unwrap();
    fs::create_dir_all(env.root()).unwrap();
    fs::write(dots.join("aliases.toml"), "version = 1\n").unwrap();
    std::os::unix::fs::symlink(dots.join("aliases.toml"), env.root().join("aliases.toml")).unwrap();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["rm", "gs"]).success();
    env.run(&["undo"]).success();
    assert!(
        fs::symlink_metadata(env.root().join("aliases.toml"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(dots.join("aliases.toml"))
            .unwrap()
            .contains("git status")
    );
}

#[cfg(unix)]
#[test]
fn editing_the_file_respects_locks() {
    let env = Env::new();
    env.run(&["add", "--lock", "deploy", "./deploy.sh"])
        .success();
    env.run(&["add", "gs", "git status"]).success();
    let editor = fake_editor(
        &env,
        "version = 1\n[aliases.gs]\ncommand = \"git status\"\n",
    );

    // Deleting the locked alias is refused; declining a retry cancels.
    env.aka()
        .env("EDITOR", &editor)
        .arg("edit")
        .write_stdin("n\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("locked"));
    assert!(env.plain_list().contains("deploy"));

    // With --force it goes through, and the deleted alias lands in the trash.
    env.aka()
        .env("EDITOR", &editor)
        .args(["edit", "--force"])
        .assert()
        .success();
    assert!(!env.plain_list().contains("deploy"));
    env.run(&["restore"])
        .success()
        .stdout(predicate::str::contains("deploy"));
}

#[cfg(unix)]
#[test]
fn edit_repairs_a_broken_alias_file() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    fs::write(env.root().join("aliases.toml"), "garbage [\n").unwrap();
    env.run(&["list"])
        .failure()
        .stderr(predicate::str::contains("aka edit"));
    let editor = fake_editor(
        &env,
        "version = 1\n[aliases.gs]\ncommand = \"git status -sb\"\n",
    );
    env.aka()
        .env("EDITOR", &editor)
        .arg("edit")
        .assert()
        .success();
    assert_eq!(env.plain_list(), "gs\tgit status -sb\n");
    // The broken version is kept as a backup
    env.run(&["undo"]).success();
    env.run(&["list"]).failure();
}

#[test]
fn import_keeps_lines_aka_does_not_cover() {
    let env = Env::new();
    env.run(&["add", "--shell", "fish", "gs", "git status"])
        .success();
    let bashrc = env.home().join(".bashrc");
    fs::write(&bashrc, "alias gs='git status'\n").unwrap();
    env.run(&[
        "import",
        "-y",
        "--clean",
        "delete",
        "--from",
        bashrc.to_str().unwrap(),
    ])
    .success();
    assert!(
        fs::read_to_string(&bashrc)
            .unwrap()
            .contains("alias gs='git status'")
    );
}

#[test]
fn trash_keeps_older_versions() {
    let env = Env::new();
    env.run(&["add", "gs", "git status"]).success();
    env.run(&["rm", "gs"]).success();
    env.run(&["add", "gs", "git status -sb"]).success();
    env.run(&["rm", "--purge", "gs"]).success();
    // purge only dropped the live alias, the older trashed one is still there
    env.run(&["restore", "gs"]).success();
    assert_eq!(env.plain_list(), "gs\tgit status\n");

    env.run(&["add", "-f", "gs", "v2"]).success();
    env.run(&["rm", "gs"]).success();
    env.run(&["add", "gs", "v3"]).success();
    env.run(&["rm", "gs"]).success();
    env.run(&["restore", "gs"]).success();
    assert_eq!(env.plain_list(), "gs\tv3\n");
    env.run(&["rm", "gs"]).success();
    env.run(&["restore"])
        .success()
        .stdout(predicate::str::contains("v2").and(predicate::str::contains("v3")));
}

#[test]
fn restore_refuses_loops() {
    let env = Env::new();
    env.run(&["add", "a", "b"]).success();
    env.run(&["rm", "a"]).success();
    env.run(&["add", "b", "a --x"]).success();
    env.run(&["restore", "a"])
        .failure()
        .stderr(predicate::str::contains("loop"));
}

#[test]
fn reserved_names_that_would_break_the_hook() {
    let env = Env::new();
    env.run(&["add", ".", "echo hi"])
        .failure()
        .stderr(predicate::str::contains("reserved"));
    env.run(&["add", "command", "echo hi"]).failure();
    env.run(&["add", "..", "cd .."]).success();
}

#[test]
fn locks_hold_for_every_command() {
    let env = Env::new();
    env.run(&["add", "--lock", "gs", "git status"]).success();
    env.run(&["cp", "gs", "gs2"]).success();
    env.run(&["disable", "gs"]).success();
    env.run(&["enable", "gs"]).success();
    env.run(&["rename", "gs", "g"])
        .failure()
        .stderr(predicate::str::contains("locked"));
    env.run(&["add", "-d", "note", "gs", "git status"])
        .failure()
        .stderr(predicate::str::contains("locked"));
    env.run(&["rm", "-f", "gs"]).success();
}

#[test]
fn config_switches_zsh_compinit_mode() {
    let env = Env::new();
    env.run(&["config"])
        .success()
        .stdout(predicate::str::contains("zsh.compinit = full (default)"));
    env.run(&["config", "set", "zsh.compinit", "fast"])
        .failure()
        .stderr(predicate::str::contains("full or cached"));
    env.run(&["config", "get", "nope"]).failure();

    let zshrc = env.home().join(".zshrc");
    fs::write(&zshrc, "export A=1\n").unwrap();
    env.run(&["setup", "-y", "--shell", "zsh"]).success();
    assert!(
        fs::read_to_string(&zshrc)
            .unwrap()
            .contains("compinit -i\n")
    );

    env.run(&["config", "set", "zsh.compinit", "cached"])
        .success()
        .stderr(predicate::str::contains("Updated the aka block"));
    env.run(&["config", "get", "zsh.compinit"])
        .success()
        .stdout("cached\n");
    assert!(
        fs::read_to_string(&zshrc)
            .unwrap()
            .contains("compinit -C -i")
    );
    // setup again keeps the chosen mode
    env.run(&["setup", "-y", "--shell", "zsh"])
        .success()
        .stderr(predicate::str::contains("already set up"));

    env.run(&["config", "unset", "zsh.compinit"]).success();
    let text = fs::read_to_string(&zshrc).unwrap();
    assert!(text.contains("compinit -i\n") && !text.contains("compinit -C"));
    assert!(
        !env.root().join("config.toml").exists(),
        "all defaults means no config file"
    );

    env.run(&["uninstall", "-y", "--shell", "zsh"]).success();
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), "export A=1\n");
}

#[test]
fn setup_can_skip_zsh_completion() {
    let env = Env::new();
    env.run(&["setup", "-y", "--no-completion", "--shell", "zsh"])
        .success();
    assert!(
        !fs::read_to_string(env.home().join(".zshrc"))
            .unwrap()
            .contains("compinit")
    );
}

#[test]
fn tags_group_aliases() {
    let env = Env::new();
    env.run(&["add", "--tag", "git", "gs", "git status"])
        .success();
    env.run(&["add", "gl", "git log", "-t", "git,log"])
        .success();
    env.run(&["add", "ll", "ls -la"]).success();
    env.run(&["add", "--tag", "Bad Tag", "x", "y"])
        .failure()
        .stderr(predicate::str::contains("valid tag"));

    env.run(&["list", "--tag", "git", "--plain"])
        .success()
        .stdout("gl\tgit log\ngs\tgit status\n");
    env.run(&["tags"])
        .success()
        .stdout(predicate::str::contains("git  (2)  gl gs"));

    env.run(&["tag", "ll", "files"]).success();
    env.run(&["show", "ll"])
        .success()
        .stdout(predicate::str::contains("tags         files"));
    env.run(&["untag", "gl", "log"]).success();
    env.run(&["tags"])
        .success()
        .stdout(predicate::str::contains("log").not());

    // a whole group on and off
    env.run(&["disable", "--tag", "git"]).success();
    let out = env.aka().args(["list", "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let enabled: Vec<bool> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["enabled"].as_bool().unwrap())
        .collect();
    assert_eq!(enabled, [false, false, true]);
    assert_eq!(v[0]["tags"], serde_json::json!(["git"]));
    env.run(&["enable", "--tag", "git"]).success();
    env.run(&["enable", "--tag", "nope"])
        .failure()
        .stderr(predicate::str::contains("no aliases are tagged"));

    // tags aren't protected by a lock, the command is
    env.run(&["lock", "gs"]).success();
    env.run(&["tag", "gs", "daily"]).success();
    env.run(&["undo"]).success();
}

#[test]
fn suggest_proposes_aliases_from_history() {
    let env = Env::new();
    let mut history = String::new();
    for i in 0..12 {
        history.push_str(&format!(": 17000000{i:02}:0;docker compose up -d\n"));
    }
    for i in 0..8 {
        history.push_str(&format!(
            ": 17000001{i:02}:0;git commit -m \"change {i}\"\n"
        ));
    }
    history.push_str(": 1700000200:0;export API_TOKEN=abc\n");
    fs::write(env.home().join(".zsh_history"), history).unwrap();

    // Enter skips: nothing is added
    env.aka()
        .arg("suggest")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("docker compose up -d").and(predicate::str::contains("dcud")),
        )
        .stdout(predicate::str::contains("TOKEN").not());
    assert_eq!(env.plain_list(), "");
    env.aka()
        .arg("suggest")
        .write_stdin("9\n")
        .assert()
        .failure()
        .stderr(predicate::str::contains("isn't one of the numbers"));

    // pick the first as suggested and the second under a name of our own
    env.aka()
        .arg("suggest")
        .write_stdin("1 2=gcm\n")
        .assert()
        .success();
    assert_eq!(
        env.plain_list(),
        "dcud\tdocker compose up -d\ngcm\tgit commit -m\n"
    );

    // both are aliases now, so there's nothing left to suggest
    env.aka()
        .arg("suggest")
        .write_stdin("\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("docker compose").not());
}
