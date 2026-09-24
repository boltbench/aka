# Changelog

## 0.1.0 (unreleased)

First version.

- Add, list, show, edit, rename, copy, remove and restore aliases
- Works in bash, zsh, fish and PowerShell, on macOS, Linux and Windows
- New aliases work in the current session right away
- Asks before replacing an existing alias
- Aliases with arguments (`$1`, `"$@"`, `$#`), translated for fish and PowerShell
- Tags: `--tag`, `aka tag`, `aka untag`, `aka tags`, and `--tag` on list, enable and disable
- `aka suggest` proposes aliases from your shell history
- Warns about shadowed commands, risky commands and syntax that won't work in some shells, and blocks alias loops
- `--confirm` aliases ask before every run; `--lock` protects an alias
- Undo, history, and a trash that keeps every removed version
- `aka import` brings in aliases from existing shell profiles
- `aka setup`, `aka uninstall`, `aka doctor` and `aka config`
- Tab completion for aka itself, alias descriptions, and completion passthrough for aliases
- `aka setup` offers to turn on zsh tab completion when it's off
- Exit codes: 0 success, 1 error, 2 cancelled
- Install scripts for macOS, Linux and Windows, and a release workflow
