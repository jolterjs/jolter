# Jolter

Jolter is a Rust-based JavaScript toolchain manager for Node.js, Bun, Deno,
and package managers.

The project is under active development. Runtime installation and switching
are functional; package-manager version installation is the next major gap
before a stable release.

## Working Features

- install Node.js, Bun, and Deno from official release sources
- resolve `latest`, Node.js `lts`, and numeric selectors such as `24.x`
- verify every downloaded archive with SHA-256
- reject non-HTTPS downloads and redirects
- install atomically under cross-process locks
- cache verified archives and release metadata
- pin project requirements in `jolter.json`
- resolve `.node-version`, `.nvmrc`, and `package.json#packageManager`
- automatically select project runtimes through self-shims
- synchronize and repair project runtime installations
- diagnose configuration, runtimes, package managers, shims, and `PATH`

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

`use`, `sync`, and `repair` generate shims for `node`, `npm`, `npx`, `pnpm`,
`yarn`, `bun`, and `deno` in the Jolter shims directory. Add that directory
to `PATH`:

```text
Windows: %USERPROFILE%\.jolter\shims
macOS/Linux: $HOME/.jolter/shims
```

Shims resolve `jolter.json`, `.node-version`, or `.nvmrc` whenever they run,
so changing directories does not require a shell hook.

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

Package-manager requirements are detected and diagnosed. Bundled Node.js
tools such as npm are usable when present, but Jolter does not yet install
requested pnpm or Yarn versions.

## Environment

- `JOLTER_HOME` overrides the default `~/.jolter` storage location.
- `JOLTER_OFFLINE=1` disables metadata requests and requires cached metadata
  and archives.

See [the architecture notes](docs/architecture.md) and
[security model](docs/security.md) for implementation details.
