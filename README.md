# A.K.A (also known as)

Manage your shell aliases from one place.

```sh
aka add gs "git status"
```

That's it. `gs` works right away in the terminal you're in, and in every new one, whether you use bash, zsh, fish or PowerShell. You never have to open `.zshrc` again.

```
$ aka add gs "git status -sb"
gs already exists:
  current  git status
  new      git status -sb
What should I do? [r]eplace / [n]ew name / [c]ancel (default: cancel):
```

## Why

Adding an alias usually means working out which file your shell reads, remembering its syntax, editing it, and reloading. Do that across a few machines and shells and they drift apart. You also get no warning when a new alias quietly replaces an old one.

A.K.A keeps every alias in one file and turns it into the right syntax for each shell.

## Features

- **One command for every shell.** bash, zsh, fish and PowerShell on macOS, Linux and Windows.
- **Works immediately.** New aliases show up in your current session, no `source ~/.zshrc` needed.
- **No surprises.** Replacing an alias asks first, and cancel is the default.
- **Safety checks.** A.K.A warns when an alias would hide a real command (`ls`, `cd`...), blocks aliases that call each other in a loop, and flags risky commands like `rm -rf`, `sudo` or `curl ... | sh`.
- **Ask before running.** `--confirm` makes an alias ask `run this? [y/N]` every time it's used.
- **Undo anything.** Every change is backed up. `aka undo` puts things back and `aka history` shows what changed.
- **Trash, not delete.** Removed aliases can be brought back with `aka restore`.
- **Lock important aliases** so they can't be replaced or removed by accident.
- **Import what you already have.** `aka import` finds the aliases in your profiles, brings them in, and tidies up the old lines if you want.
- **Tab completion everywhere.** `aka rm <TAB>` lists your aliases, and `gco <TAB>` completes branches just like `git checkout <TAB>` would.
- **Fast.** Your shell sources a pre-built file on startup, so no extra program runs when a terminal opens.

## Install

A.K.A is written in Rust. Prebuilt binaries and package manager installs (Homebrew, Scoop, winget) are coming with the first release. For now, build it from source:

```sh
git clone https://github.com/zeuslcf/aka.git
cd a.k.a
cargo install --path .
```

Then hook it into your shells, once:

```sh
aka setup
```

`aka setup` finds the shells you have installed and adds a small, clearly marked block to each profile. It shows you exactly which files it will touch and asks before changing anything. If zsh's tab completion is off, it offers to turn that on too.

## Usage

```sh
aka add gs "git status"                      # add an alias
aka add ll ls -la                            # quotes are optional
aka add gs "git status" -d "quick status"    # with a description
aka add open-here "open ." --os macos        # only on macOS
aka add nuke "rm -rf build" --confirm        # ask before every run
aka add deploy "./deploy.sh" --lock          # protect it

aka list                                     # or just `aka`
aka list git                                 # filter by name, command or description
aka list --json                              # for scripts
aka show gs                                  # everything about one alias

aka edit gs                                  # change one alias
aka edit                                     # open the whole file in $EDITOR
aka rename gs gst
aka cp gs gss
aka disable gs                               # keep it, but turn it off
aka enable gs

aka rm gs                                    # goes to the trash
aka restore                                  # see the trash
aka restore gs                               # bring it back

aka undo                                     # revert the last change
aka history

aka import                                   # bring in aliases from your profiles
aka doctor                                   # check that everything is healthy
```

A note on options: put them before the command (`aka add -d "note" gs git status`) or after a quoted command (`aka add gs "git status" -d "note"`). Without quotes, everything after the name counts as part of the command, so `aka add ll ls -d` keeps the `-d` for `ls`.

### Global flags

| Flag | What it does |
|---|---|
| `-y`, `--yes` | Answer yes to every question |
| `-f`, `--force` | Skip safety questions and override locks |
| `--dry-run` | Show what would change without saving anything |

## How it works

A program can't change the aliases of the shell that started it, so A.K.A doesn't try. Instead:

1. Your aliases live in `~/.config/aka/aliases.toml` (`%APPDATA%\aka\` on Windows). Set `AKA_HOME` to put them somewhere else.
2. Every change rewrites a small init file for each shell: `init.bash`, `init.zsh`, `init.fish` and `init.ps1`.
3. `aka setup` adds one line to your profile that sources the right init file.
4. The init file also defines an `aka` shell function that reloads it after every command. That's how new aliases show up in your current terminal. PowerShell reloads from its prompt instead.

The alias file is plain TOML, so it's easy to read, back up, or keep in your dotfiles repo:

```toml
version = 1

[aliases.gs]
command = "git status"
description = "quick status"

[aliases.nuke]
command = "rm -rf build"
confirm = true
```

If you'd rather not have A.K.A touch your profile, skip `aka setup` and add this line yourself:

```sh
eval "$(aka init zsh)"      # or bash
aka init fish | source      # fish
```

## Shell notes

- **PowerShell** can't pass arguments through `Set-Alias`, so A.K.A defines each alias as a small function. Built-in aliases with the same name (like `gp` or `ls`) are replaced for the session.
- **Commands run in the shell that uses them.** An alias like `ls -la` means something different in PowerShell. Use `--shell` to limit an alias to the shells where it makes sense. `aka import` does this for you automatically.
- **zsh tab completion** needs zsh's completion system (`compinit`). Frameworks like oh-my-zsh turn it on for you, but a plain `.zshrc` often doesn't. `aka setup` checks, and if it's off, offers to turn it on inside aka's own block, along with Homebrew's completion folder on macOS. That adds about 30ms to shell startup, and `aka uninstall` takes it away again. Use `aka setup --no-completion` to skip it.

## Uninstall

```sh
aka uninstall            # removes the hook from your profiles, keeps your aliases
aka uninstall --purge    # also deletes your aliases, trash and backups
```

Then remove the binary with whatever you installed it with (`cargo uninstall aka`).

## Roadmap

**v0.1 (now):** everything above.

**v0.2:** tags and groups, fuzzy search, profiles (`aka profile use work`), aliases with arguments (`mkcd` → `mkdir -p $1 && cd $1`), an interactive picker, export to Markdown or JSON, and conditional aliases (`--requires eza`).

**v0.3:** sync across machines through git or a gist, `aka suggest` for long commands you type often, "you have an alias for that" hints, fish/zsh abbreviation mode, usage stats, and per-project aliases.

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT. See [LICENSE](LICENSE).
