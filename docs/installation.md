# Installation

Jolter provides public installation entry points for Windows and Unix-like
systems, plus checksum-protected release archives and source builds.

## Quick Install

### Windows

Open PowerShell and run:

```powershell
irm https://jolter.dev/win/install | iex
```

### Linux and macOS

Open a terminal and run:

```bash
curl -fsSL https://jolter.dev/unix/install | sh
```

The endpoints above are the canonical installer locations. This repository
does not define additional public script flags, so do not depend on
undocumented arguments in automation.

## Inspect Before Running

Piping a remote script directly into a shell trusts the current content at
that URL. Environments with stricter controls can download and inspect it
first.

Windows:

```powershell
Invoke-WebRequest https://jolter.dev/win/install -OutFile install-jolter.ps1
Get-Content .\install-jolter.ps1
.\install-jolter.ps1
```

Linux or macOS:

```bash
curl -fsSL https://jolter.dev/unix/install -o install-jolter.sh
less install-jolter.sh
sh install-jolter.sh
```

Use normal endpoint-security and code-review policy before executing any
downloaded installer.

## Complete Shell Setup

After installation:

```bash
jolter --version
jolter setup
```

`setup` configures shell PATH integration and refreshes shims matching installed runtimes and tools, then prints exact
commands for the current shell and persistent user configuration. Apply the
persistent command, restart the terminal, and run:

```bash
jolter doctor
```

Continue with [getting started](getting-started.md).

## Supported Release Platforms

Tagged releases currently publish:

| Platform | Architecture  | Rust target                |
| -------- | ------------- | -------------------------- |
| Windows  | x64           | `x86_64-pc-windows-msvc`   |
| Linux    | x64           | `x86_64-unknown-linux-gnu` |
| macOS    | Intel x64     | `x86_64-apple-darwin`      |
| macOS    | Apple Silicon | `aarch64-apple-darwin`     |

WSL uses the Linux x64 build. Runtime providers can impose additional
requirements; for example, the available Bun x64 artifact may require CPU
features defined by Bun.

## Manual Release Archive

Download the archive and sibling `.sha256` file from the matching GitHub
release. Archive names follow:

```text
jolter-v<version>-<rust-target>.tar.gz
jolter-v<version>-x86_64-pc-windows-msvc.zip
```

Verify before extraction.

Linux:

```bash
sha256sum -c jolter-v0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

macOS:

```bash
shasum -a 256 -c jolter-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

Windows PowerShell:

```powershell
$actual = (Get-FileHash `
  .\jolter-v0.2.0-x86_64-pc-windows-msvc.zip `
  -Algorithm SHA256).Hash.ToLowerInvariant()
$actual
Get-Content .\jolter-v0.2.0-x86_64-pc-windows-msvc.zip.sha256
```

Compare the values, extract the archive, and place `jolter` or `jolter.exe` in
a user-controlled directory on `PATH`. Then run `jolter setup`.

Never install an archive whose checksum does not match.

## Build From Source

Rust `1.85` or newer is required:

```bash
git clone https://github.com/jolterjs/jolter.git
cd jolter
make build-release
```

The executable is written to:

```text
Windows: target/release/jolter.exe
Unix:   target/release/jolter
```

Run the repository quality gates before distributing a local build:

```bash
make fmt-check
make clippy
make test
```

## First Toolchain

```bash
jolter setup
jolter use node@lts
jolter doctor
node --version
```

Install a managed tool after Node.js is active:

```bash
jolter use pnpm@10
pnpm --version
```

## Upgrade

Run the current installer entry point again or manually replace the executable
with a verified newer release archive. After upgrading:

```bash
jolter --version
jolter setup
jolter doctor
```

Refreshing shims ensures they point to the new Jolter executable. Review the
[changelog](../CHANGELOG.md) and
[compatibility policy](compatibility.md) before moving between minor versions.

## Removing Jolter

Jolter does not currently expose a self-uninstall command. Locate the installed
executable:

Windows:

```powershell
Get-Command jolter
```

Linux or macOS:

```bash
command -v jolter
```

Remove that executable using your normal system policy. Jolter-managed data is
stored under `%USERPROFILE%\.jolter` or `$HOME/.jolter` unless `JOLTER_HOME`
overrides it. Inspect that directory before deleting it; it contains installed
runtimes, tools, cache data, shims, and active selections.

Removing Jolter does not modify project `jolter.json` files.

## Installation Troubleshooting

If `jolter` is not found:

1. restart the shell;
2. confirm the binary directory is on `PATH`;
3. locate the binary with `Get-Command` or `command -v`;
4. invoke it by absolute path and run `jolter setup`;
5. run `jolter doctor` after applying the printed `PATH` command.

For proxy, TLS, permission, and offline failures, use
[networking and offline mode](networking-and-offline.md) and
[troubleshooting](troubleshooting.md).
