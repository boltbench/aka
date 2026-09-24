# Releasing aka

## Cutting a release

1. Update the version in `Cargo.toml` and add a `## x.y.z` section to `CHANGELOG.md`.
2. Commit, then tag and push:
   ```sh
   git tag v0.1.0
   git push origin main v0.1.0
   ```
3. The `Release` workflow builds macOS, Linux and Windows binaries (Intel and ARM each), packages them with checksums, and publishes a GitHub release. Its notes come from the changelog section for that version.

The release includes `install.sh`, `install.ps1` and a `checksums.txt`, so these work straight away:

```sh
curl -fsSL https://github.com/boltbench/aka/releases/latest/download/install.sh | sh
```

```powershell
irm https://github.com/boltbench/aka/releases/latest/download/install.ps1 | iex
```

## Package managers

These are off until you set them up once. Each one only needs a repository and a token.

### Homebrew

1. Create a public repo called `boltbench/homebrew-tap`.
2. Create a fine-grained token with write access to that repo only, and add it to this repo as the secret `HOMEBREW_TAP_TOKEN`.
3. Add the repository variable `HOMEBREW_TAP_ENABLED` = `true`.

Every release then updates `Formula/aka.rb`, and people install with:

```sh
brew install boltbench/tap/aka
```

### Scoop

1. Create a public repo called `boltbench/scoop-bucket`.
2. Add a token with write access to it as the secret `SCOOP_BUCKET_TOKEN`.
3. Add the repository variable `SCOOP_BUCKET_ENABLED` = `true`.

```powershell
scoop bucket add boltbench https://github.com/boltbench/scoop-bucket
scoop install aka
```

### winget

The first version has to go in by hand, since the winget maintainers review new packages:

```powershell
winget install wingetcreate
wingetcreate new https://github.com/boltbench/aka/releases/download/v0.1.0/aka-v0.1.0-x86_64-pc-windows-msvc.zip
```

Use `boltbench.aka` as the package identifier. Once it's accepted, add a classic token with `public_repo` scope as the secret `WINGET_TOKEN` and set the variable `WINGET_ENABLED` = `true`. Later releases are then submitted automatically.

## Checking the packaging locally

`render.sh` prints the Homebrew formula or Scoop manifest for a release:

```sh
packaging/render.sh homebrew 0.1.0
packaging/render.sh scoop 0.1.0
```

Set `AKA_CHECKSUMS` to a local `checksums.txt` to try it without a published release.
