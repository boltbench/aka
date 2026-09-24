# Changelog

## 0.1.0 (unreleased)

First version.

- Add, list, show, edit, rename, copy, remove and restore aliases
- Works in bash, zsh, fish and PowerShell, on macOS, Linux and Windows
- New aliases work in the current session right away
- Asks before replacing an existing alias
- Warns about shadowed commands and risky commands, and blocks alias loops
- `--confirm` aliases ask before every run; `--lock` protects an alias
- Undo, history, and a trash for removed aliases
- `aka import` brings in aliases from existing shell profiles
- `aka setup`, `aka uninstall` and `aka doctor`
- Tab completion for aka itself, plus completion passthrough for aliases
- `aka setup` offers to turn on zsh tab completion when it's off
