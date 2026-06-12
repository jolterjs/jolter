# Jolter

Jolter is a Rust-based JavaScript toolchain manager for Node.js, Bun, Deno,
and package managers.

The project is under active development. Runtime and package-manager
installation, synchronization, repair, and project-aware switching are
functional. Cross-platform release hardening and onboarding are the next major
steps before a stable release.

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
- accept exact Corepack descriptors with SHA-1 or SHA-2 hashes
- validate package-manager Node.js engine requirements before installation
- automatically select project runtimes through self-shims
- dispatch managed package managers through the selected project Node.js
- synchronize and repair project runtime installations
- install shims and print exact shell-specific `PATH` setup commands
- list runtimes and managed package managers with installation health
- diagnose configuration, runtimes, package managers, shims, and `PATH`
- emit machine-readable diagnostics with `jolter doctor --json`

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
jolter doctor --json
```

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
  "runtime": {
    "node": "24.x"
  },
  "packageManager": {
    "pnpm": "10.x"
  }
}
```

Package-manager requirements for npm, pnpm, and Yarn are installed into
`~/.jolter/tools` and selected by the generated shims. Jolter verifies npm
registry packages with their SHA-512 integrity metadata and checks the
additional hash when `packageManager` uses an exact Corepack descriptor such
as `pnpm@10.12.1+sha224.<hash>`. The selected package manager must also support
the project's selected Node.js version according to its `engines.node` range.

## Environment

- `JOLTER_HOME` overrides the default `~/.jolter` storage location.
- `JOLTER_OFFLINE=1` disables metadata requests and requires cached metadata
  and archives.

See [shell setup](docs/setup.md), [diagnostics](docs/diagnostics.md),
[the architecture notes](docs/architecture.md), and the
[security model](docs/security.md) for implementation details. The
[project roadmap](ROADMAP.md) records feature status, known gaps, and release
milestones.
