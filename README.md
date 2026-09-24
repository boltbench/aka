# A.K.A (also known as)

[![CI](https://github.com/boltbench/aka/actions/workflows/ci.yml/badge.svg)](https://github.com/boltbench/aka/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/boltbench/aka)](https://github.com/boltbench/aka/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Manage your shell aliases from one place.

```sh
aka add gs "git status"
```

That's it. `gs` works right away in the terminal you're in, and in every new one, whether you use bash, zsh, fish or PowerShell. You never have to open `.zshrc` again.

![aka adding an alias, catching a conflict, listing and undoing](docs/demo.gif)

## Why

Adding an alias usually means working out which file your shell reads, remembering its syntax, editing it, and reloading. Do that across a few machines and shells and they drift apart. You also get no warning when a new alias quietly replaces an old one.

A.K.A keeps every alias in one file and turns it into the right syntax for each shell.

## Features

- **One command for every shell.** bash, zsh, fish and PowerShell on macOS, Linux and Windows.
- **Works immediately.** New aliases show up in your current session, no `source ~/.zshrc` needed.
- **No surprises.** Replacing an alias asks first, and cancel is the default.
- **Aliases with arguments.** `aka add mkcd 'mkdir -p "$1" && cd "$1"'` just works, in every shell.
- **Suggestions from your history.** `aka suggest` finds the long commands you type most and proposes short names for them.
- **Tags.** Group aliases (`--tag git`), list a group, or switch a whole group on and off.
- **Safety checks.** A.K.A warns when an alias would hide a real command (`ls`, `cd`...), blocks aliases that call each other in a loop, flags risky commands like `rm -rf`, `sudo` or `curl ... | sh`, and points out syntax that won't work in some of your shells.
- **Ask before running.** `--confirm` makes an alias ask `run this? [y/N]` every time it's used.
- **Undo anything.** Every change is backed up. `aka undo` puts things back and `aka history` shows what changed.
- **Trash, not delete.** Removed aliases can be brought back with `aka restore`.
- **Lock important aliases** so they can't be replaced or removed by accident.
- **Import what you already have.** `aka import` finds the aliases in your profiles, brings them in, and tidies up the old lines if you want.
- **Tab completion everywhere.** `aka rm <TAB>` lists your aliases with their descriptions, and `gco <TAB>` completes branches just like `git checkout <TAB>` would.
- **Fast.** Your shell sources a pre-built file on startup, so no extra program runs when a terminal opens.
- **Private.** A.K.A never connects to the network and collects no data.

## Install

**macOS and Linux**

```sh
curl -fsSL https://github.com/boltbench/aka/releases/latest/download/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://github.com/boltbench/aka/releases/latest/download/install.ps1 | iex
```

Both scripts download the right binary for your system, check its checksum, and install it (to `~/.local/bin`, or `%LOCALAPPDATA%\Programs\aka` on Windows). You can also grab a binary from the [releases page](https://github.com/boltbench/aka/releases), or build from source with Rust:

```sh
cargo install --git https://github.com/boltbench/aka
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
aka add gl "git log --oneline" --tag git     # with a tag
aka add open-here "open ." --os macos        # only on macOS
aka add nuke "rm -rf build" --confirm        # ask before every run
aka add deploy "./deploy.sh" --lock          # protect it
aka add mkcd 'mkdir -p "$1" && cd "$1"'      # takes arguments

aka list                                     # or just `aka`
aka list git                                 # filter by name, command or description
aka list --tag git                           # only one tag
aka list --json                              # for scripts
aka show gs                                  # everything about one alias

aka edit gs                                  # change one alias
aka edit                                     # open the whole file in $EDITOR
aka rename gs gst
aka cp gs gss
aka disable gs                               # keep it, but turn it off
aka enable --tag git                         # turn a whole tag back on

aka tag gs git daily                         # add tags
aka untag gs daily
aka tags                                     # list your tags

aka rm gs                                    # goes to the trash
aka restore                                  # see the trash
aka restore gs                               # bring it back

aka undo                                     # revert the last change
aka history

aka suggest                                  # ideas from your shell history
aka import                                   # bring in aliases from your profiles
aka doctor                                   # check that everything is healthy
aka config                                   # see and change settings
```

A note on options: put them before the command (`aka add -d "note" gs git status`) or after a quoted command (`aka add gs "git status" -d "note"`). Without quotes, everything after the name counts as part of the command, so `aka add dr docker run -d nginx` keeps the `-d` for docker. If it looks like you meant an option for aka, it tells you.

### Aliases with arguments

Use `$1`, `$2`... for single arguments, `"$@"` for all of them, and `$#` for how many there are. Quote the command with single quotes so your shell doesn't fill in `$1` while you type it:

```sh
aka add mkcd 'mkdir -p "$1" && cd "$1"'
aka add grepall 'grep -rn "$@" .'
```

A.K.A turns these into functions and translates them for fish (`$argv[1]`) and PowerShell (`$args[0]`).

### Suggestions

```
$ aka suggest
 #  USED  COMMAND                NAME
 1  42    git status -sb         gss
 2  17    docker compose up -d   dcud
Add which? Numbers like `1 3`, `2=dcu` to pick the name, `all`, or Enter to skip:
```

It reads your zsh, bash, fish and PowerShell history on your machine, only reads it, and skips anything that looks like it contains a password or token.

### Global flags

| Flag | What it does |
|---|---|
| `-y`, `--yes` | Answer yes to every question |
| `-f`, `--force` | Skip safety questions and override locks |
| `--dry-run` | Show what would change without saving anything |

### Exit codes

`0` means it worked, `1` means an error, and `2` means you cancelled or declined at a prompt.

## How it works

A program can't change the aliases of the shell that started it, so A.K.A doesn't try. Instead:

1. Your aliases live in `~/.config/aka/aliases.toml` (`%APPDATA%\aka\` on Windows). Set `AKA_HOME` to put them somewhere else.
2. Every change rewrites a small init file for each shell: `init.bash`, `init.zsh`, `init.fish` and `init.ps1`.
3. `aka setup` adds one line to your profile that sources the right init file.
4. The init file also defines an `aka` shell function that reloads it after every command. That's how new aliases show up in your current terminal. PowerShell reloads from its prompt instead.

The alias file is plain TOML, so it's easy to read, back up, or keep in your dotfiles repo (symlinks are kept as they are):

```toml
version = 1

[aliases.gs]
command = "git status"
description = "quick status"
tags = ["git"]

[aliases.nuke]
command = "rm -rf build"
confirm = true
```

If you'd rather not have A.K.A touch your profile, skip `aka setup` and add this line yourself:

```sh
eval "$(aka init zsh)"      # or bash
aka init fish | source      # fish
```

## Settings

```sh
aka config                              # list settings
aka config set zsh.compinit cached      # change one
aka config unset zsh.compinit           # back to the default
```

| Setting | Values | What it does |
|---|---|---|
| `zsh.compinit` | `full` (default), `cached` | When `aka setup` turned on zsh completion, `full` checks for new completions on every start (about 30ms). `cached` checks once a day instead (about 5ms). |

## Shell notes

- **PowerShell** can't pass arguments through `Set-Alias`, so A.K.A defines each alias as a small function, and Tab still completes like the command it runs. Built-in aliases with the same name (like `gp` or `ls`) are replaced for the session. If a prompt tool like starship or oh-my-posh loads after aka's block, new aliases only show up in new windows; `aka doctor` spots this and `aka setup` moves the block.
- **Commands run in the shell that uses them.** An alias like `ls -la` means something different in PowerShell. A.K.A warns when a command looks like it only works in some shells, and `--shell` limits an alias to the shells where it makes sense. `aka import` does this for you automatically.
- **zsh tab completion** needs zsh's completion system (`compinit`). Frameworks like oh-my-zsh turn it on for you, but a plain `.zshrc` often doesn't. `aka setup` checks, and if it's off, offers to turn it on inside aka's own block, along with Homebrew's completion folder on macOS. `aka uninstall` takes it away again, and `aka setup --no-completion` skips it.

## Privacy

A.K.A works entirely on your machine. It never connects to the network, sends nothing anywhere, and collects no usage data. The only downloads are the ones the install scripts make when you run them.

## Uninstall

```sh
aka uninstall            # removes the hook from your profiles, keeps your aliases
aka uninstall --purge    # also deletes your aliases, trash and backups
```

Then delete the `aka` binary (from `~/.local/bin`, or with `cargo uninstall aka` if you built it).

## Roadmap

**Next:** Homebrew, Scoop and winget packages, fuzzy search, profiles (`aka profile use work`), an interactive picker, export to Markdown or JSON, and conditional aliases (`--requires eza`).

**Later:** sync across machines through git or a gist, "you have an alias for that" hints, fish/zsh abbreviation mode, usage stats, and per-project aliases.

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md). To report a security problem, see [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
