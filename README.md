# Jolter

Jolter is a Rust-based JavaScript toolchain manager for Node.js, Bun, Deno,
and package managers.

Version `0.2.0` implements the stable-core command surface. The project remains
pre-1.0 until its configured cross-platform and provider-smoke workflows have
passed through a release-candidate period.

## Working Features

- install Node.js, Bun, and Deno from official release sources
- resolve `latest`, Node.js `lts`, and numeric selectors such as `24.x`
- verify runtime archives with SHA-256 and registry packages with SHA-512
- reject non-HTTPS downloads and redirects
- install atomically under cross-process locks
- cache verified archives and release metadata
- pin project requirements in `jolter.json`
- resolve `.node-version`, `.nvmrc`, and `package.json#packageManager`
- install verified npm, pnpm, and Yarn versions from the npm registry
- manually activate npm, pnpm, and Yarn versions through `jolter use`
- accept exact Corepack descriptors with SHA-1 or SHA-2 hashes
- validate package-manager Node.js engine requirements before installation
- automatically select project runtimes through self-shims
- dispatch managed package managers through the selected project Node.js
- synchronize and repair project runtime installations
- install shims and print exact shell-specific `PATH` setup commands
- list runtimes and managed package managers with installation health
- diagnose configuration, runtimes, package managers, shims, and `PATH`
- probe installed versions with a bounded timeout and validate manifests
- detect shadowing by system tools, nvm, fnm, Volta, and Corepack paths
- emit machine-readable diagnostics with `jolter doctor --json`
- uninstall exact versions, prune old versions, and clean caches safely
- prepare project toolchains for CI with `jolter setup-ci`
- generate Bash, Zsh, Fish, Elvish, and PowerShell completions
- show live runtime and package-manager download progress with current actions
- provide deterministic CI output through global terminal-control flags
- enforce at least 80% line coverage in CI

## Build

```bash
cargo build --release
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

The executable is written to `target/release/jolter` or
`target/release/jolter.exe`.

## Usage

Install and activate a runtime:

```bash
jolter use node@24
jolter use node@lts
jolter use bun@1.3
jolter use deno@2
```

Install and activate a package manager using the active Node.js runtime:

```bash
jolter use npm@11
jolter use pnpm@10
jolter use yarn@4
```

Pin and synchronize a project:

```bash
jolter pin node@24
jolter sync
jolter doctor
```

Set up command shims and receive instructions for the current shell:

```bash
jolter setup
jolter setup --shell powershell
jolter setup --shell bash
```

Inspect installed toolchains or consume diagnostics as JSON:

```bash
jolter list
jolter list --json
jolter doctor --json
```

Manage storage lifecycle:

```bash
jolter uninstall node@24.1.0
jolter prune --dry-run
jolter prune --keep 2
jolter cache status
jolter cache clean
```

Prepare CI and generate shell completions:

```bash
jolter setup-ci --json
jolter completions bash
jolter completions powershell
```

Control terminal output globally:

```bash
jolter use node@24 --no-progress
jolter sync --no-color
jolter setup-ci --quiet
jolter use pnpm@10 --verbose
```

Interactive terminals receive a single updating `[jolter]` line with the current
action, download bytes, percentage, and transfer rate. CI and redirected
output automatically use stable line-oriented logs. Progress is written to
standard error, while final results and JSON remain on standard output.
ANSI color is emitted only when the terminal advertises support, so legacy
Windows Command Prompt sessions automatically fall back to clean plain text.
The `list` command calculates its status and path columns from the installed
tool names instead of relying on terminal tab stops.

`setup`, `use`, `sync`, and `repair` generate shims for `node`, `npm`, `npx`,
`pnpm`, `yarn`, `bun`, and `deno` in the Jolter shims directory:

```text
Windows: %USERPROFILE%\.jolter\shims
macOS/Linux: $HOME/.jolter/shims
```

Shims resolve `jolter.json`, `.node-version`, or `.nvmrc` whenever they run,
so changing directories does not require a shell hook.

See [shell setup](docs/setup.md) for supported shells and persistence behavior.

## Configuration

```json
{
  "schemaVersion": 1,
  "runtime": {
    "node": "24.x"
  },
  "packageManager": {
    "pnpm": "10.x"
  }
}
```

Package-manager requirements for npm, pnpm, and Yarn are installed into
`~/.jolter/tools` and selected by the generated shims. A project requirement
has priority over a globally active version selected with `jolter use`. Jolter
verifies npm registry packages with their SHA-512 integrity metadata and checks
the additional hash when `packageManager` uses an exact Corepack descriptor
such as `pnpm@10.12.1+sha224.<hash>`. The selected package manager must also
support the selected Node.js version according to its `engines.node` range.

## Environment

- `JOLTER_HOME` overrides the default `~/.jolter` storage location.
- `JOLTER_OFFLINE=1` disables metadata requests and requires cached metadata
  and archives.
- `JOLTER_NO_PROGRESS=1` disables in-place progress updates.
- `JOLTER_NO_COLOR=1` and the standard `NO_COLOR` variable disable ANSI color.

See [shell setup](docs/setup.md), [diagnostics](docs/diagnostics.md),
[the architecture notes](docs/architecture.md), and the
[security model](docs/security.md) for implementation details. The
[project roadmap](ROADMAP.md) records feature status, known gaps, and release
milestones.

Additional guides:

- [installation](docs/installation.md)
- [CI integration](docs/ci.md)
- [storage lifecycle](docs/lifecycle.md)
- [compatibility policy](docs/compatibility.md)
- [troubleshooting](docs/troubleshooting.md)
