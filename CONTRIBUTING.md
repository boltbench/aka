# Contributing

Thanks for wanting to help. Bug reports, ideas and pull requests are all welcome.

## Getting started

You need a recent stable Rust (1.88 or newer).

```sh
cargo build
cargo test
```

The tests never touch your real home directory. Each one runs against a temporary `HOME`.

Some tests source the generated init files in real shells (bash, zsh, fish, PowerShell). A test is skipped when its shell isn't installed, and CI runs all of them on macOS, Linux and Windows.

## Before you open a pull request

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

If you change what an init file looks like, the snapshot tests will fail. Look at the new output, and if it's what you meant, accept it:

```sh
cargo install cargo-insta   # once
cargo insta review
```

## Where things live

| Path | What's in it |
|---|---|
| `src/cli.rs` | Command line arguments |
| `src/commands/` | One module per group of commands |
| `src/shells.rs` | Generates the init file for each shell |
| `src/store.rs` | Loading, saving, locking, backups and undo |
| `src/safety.rs` | Name checks, shadowing, loops and risky commands |
| `src/setup.rs` | Finding profiles and adding or removing the hook |
| `src/import.rs` | Reading aliases out of existing profiles |
| `tests/cli.rs` | End-to-end tests |

## Reporting a bug

Please include your OS, your shell and its version, the output of `aka doctor`, and the steps that reproduce the problem.
