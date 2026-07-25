# Jolter Roadmap

Last updated: July 25, 2026

This roadmap is the implementation plan and records the repository's
current development state. It is intended to be the source of truth 
for project status, sequencing, and release readiness.

## Project Position

Current version: `0.2.0` (`0.3.0` release-candidate preparation)

Current maturity: **beta / release-candidate preparation**

Current phase: **stable-core implementation complete; WASM plugin system, development toolkit, and hosted plugin registry operational; release validation in progress**

Jolter can currently resolve, securely install, activate, pin, synchronize,
repair, and dispatch JavaScript runtimes. It can also manually activate or
project-pin npm, pnpm, and Yarn versions, as well as third-party custom tools provided by WebAssembly (WASM) plugins. The main architecture exists as a 10-crate Rust workspace, supported by a full developer toolkit (`@jolter/jdt`), a hosted plugin registry API, a catalog Web UI, and automated release publication workflows.

Jolter includes the stable-core command surface, Schema v2 project configuration, multiple pinned tools and plugins, active-version updates, WASM component sandboxing (`wasmtime`), deeper diagnostics, storage lifecycle commands, CI setup, release workflows, and measured test coverage above the target. Additional tool providers and plugin host architecture have progressed from future goals to fully implemented features.

## Status Legend

- **Complete**: implemented and covered by automated tests.
- **Partial**: useful implementation exists, but one or more specified
  behaviors or release-quality requirements remain.
- **Planned**: accepted scope with no complete implementation yet.
- **Future**: intentionally deferred until after the stable core.

## Current Features

### Runtime Management

Status: **Complete for the core engine**

- Node.js, Bun, and Deno runtime kinds.
- Numeric selectors such as `node@24`, `bun@1.3`, and `deno@2`.
- Wildcard selectors such as `node@24.x`.
- `latest` for all supported runtimes.
- `lts` for Node.js.
- Official Node.js release index integration.
- Official Bun and Deno GitHub release integration.
- Platform artifact selection for Windows, Linux, and macOS.
- x64 and ARM64 artifact selection.
- Bun baseline CPU selection on x64 where required.
- Local runtime reuse without a network request for numeric selectors.
- Exact active runtime persistence.

### Secure Installation

Status: **Complete for runtimes, tools, and plugins**

- HTTPS-only metadata and artifact requests.
- HTTPS-only redirect policy.
- SHA-256 verification for Node.js, Bun, Deno, and WASM plugin binaries.
- SHA-512 Subresource Integrity verification for npm registry packages.
- Optional Corepack descriptor hash verification in addition to registry SRI.
- Bounded metadata, archive, entry-count, and extracted-size handling.
- Archive path traversal protection.
- Symlink and hard-link validation.
- Temporary staging followed by atomic publication.
- Cross-process installation locks.
- Verified artifact caching.
- Cached release metadata with offline fallback.
- `JOLTER_OFFLINE=1` support.
- No downloaded executable is run during installation.
- Installation manifests for runtimes, managed tools, and WASM plugins.

### Tool & Plugin Management

Status: **Complete for built-in tools (npm, pnpm, Yarn) and WASM plugins**

- `jolter.json` tool and plugin requirements (Schema Version 2).
- `package.json#packageManager` discovery.
- npm installation from the official npm registry package.
- pnpm installation from the official npm registry package.
- Yarn installation from the official `@yarnpkg/cli-dist` package.
- Custom tool provision via WASM plugins hosted on the Jolter Registry or local store.
- Numeric, wildcard, and `latest` selectors for tools and plugins.
- Exact Corepack-style descriptors with SHA-1 and SHA-2 hashes.
- Highest matching stable version selection.
- npm-style Node.js engine compatibility validation before installation and local reuse.
- Verified, cached, atomic installation under `~/.jolter/tools/` and `~/.jolter/plugins/`.
- Project-aware dispatch through `npm`, `npx`, `pnpm`, `yarn`, and plugin tool shims.
- Manual installation and global activation through `jolter use` and `jolter plugin install`.
- Exact active tool and plugin persistence and inventory markers.
- Multi-tool and plugin synchronization and repair (`jolter sync --yes`).
- Doctor checks for matching managed installations and WASM plugin integrity.

Known limitations:

- npm registry selection is currently fixed to the public official registry.
- Tool version execution is probed by `doctor`; Jolter also validates the exact managed installation selected from verified metadata.
- Bun's bundled package manager is available through the Bun runtime, but it is not a separately versioned managed tool.

### Project Configuration

Status: **Complete for Schema Version 2**

- Strict `jolter.json` parsing with unknown-field rejection.
- Atomic configuration writes using temporary files.
- Support for Schema Version 2 (`https://schemas.jolter.dev/project/v2/schema.json`).
- One runtime requirement per project.
- Runtime, tool, and plugin requirements per project.
- Runtime and tool pinning through `jolter pin`.
- Plugin pinning through `jolter pin` and `jolter plugin install`.
- Pinning preserves unrelated runtime, tool, and plugin requirements.
- Legacy `packageManager` input compatibility with canonical `tools` output.
- Configuration discovery from nested project directories.

Current schema (v2):

```json
{
  "$schema": "https://schemas.jolter.dev/project/v2/schema.json",
  "schemaVersion": 2,
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "eslint": "8.x",
    "pnpm": "10.x",
    "yarn": "4.x"
  },
  "plugins": {
    "eslint": "1.x"
  }
}
```

### Requirement Resolution

Status: **Complete**

Runtime priority:

1. `jolter.json`
2. `.node-version`
3. `.nvmrc`

Tool & Plugin priority:

1. `jolter.json`
2. `package.json#packageManager`

Runtime, tool, and plugin requirements are resolved independently, allowing a project to combine a Node version file with tool declarations and WASM plugins in `jolter.json`.

### Commands

| Command                           | Status                         | Current behavior                                                                                                                    |
| --------------------------------- | ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------- |
| `jolter use <tool>`               | Complete                       | Installs or reuses a runtime, tool, or plugin tool, activates it, and installs shims. Tools require active Node.js.                 |
| `jolter pin <runtime-or-tool>`    | Complete                       | Writes a runtime, tool, or plugin tool requirement to `jolter.json` while preserving other entries.                                  |
| `jolter update [target]`          | Complete                       | Updates an active runtime or tool within its major line, accepts explicit selectors, and supports `--all`.                          |
| `jolter plugin install <target>`  | Complete                       | Resolves, downloads, verifies SHA-256 integrity, and installs a WASM plugin globally from the Jolter Registry.                     |
| `jolter plugin list [--json]`     | Complete                       | Lists installed WASM plugins, canonical names, versions, paths, and status in table or JSON format.                                 |
| `jolter plugin update [name]`     | Complete                       | Updates installed WASM plugins to their highest matching version selectors, supporting single targets or `--all`.                   |
| `jolter plugin remove <name>`     | Complete                       | Safely removes an installed WASM plugin with active shim protection.                                                                |
| `jolter setup`                    | Complete                       | Installs shims and prints exact current-session and persistent `PATH` commands for PowerShell, Command Prompt, Bash, Zsh, and Fish. |
| `jolter list`                     | Complete for current artifacts | Lists runtimes, managed tools, and installed plugins, active markers, paths, and readiness.                                         |
| `jolter doctor`                   | Complete for stable core       | Probes versions, validates manifests, WASM component integrity, engine compatibility, detects conflicts, and emits JSON/text.     |
| `jolter repair`                   | Complete for current artifacts | Reinstalls incomplete runtime, tool, and plugin installations (`--yes` auto-installs missing plugins).                               |
| `jolter sync`                     | Complete for current schema    | Synchronizes project runtimes, tools, and plugins (`--yes` auto-installs missing plugins).                                          |
| `jolter uninstall <tool@version>` | Complete                       | Removes exact versions with active-toolchain protection and shared installer locks.                                                 |
| `jolter prune`                    | Complete                       | Preserves active, project-required, and configured newest versions; supports dry runs.                                              |
| `jolter cache status/clean`       | Complete                       | Inspects and safely cleans metadata and artifact caches.                                                                            |
| `jolter setup-ci`                 | Complete                       | Synchronizes exact versions (`--yes` for plugins), reports cache/shim paths, emits JSON and GitHub Actions outputs.                 |
| `jolter completions`              | Complete                       | Generates Bash, Zsh, Fish, Elvish, and PowerShell completions.                                                                      |

### Automatic Switching and Shims

Status: **Complete for invocation-time switching**

- Self-shims for `node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, `deno`, and custom plugin-provided tools.
- Runtime and plugin tool selection is evaluated whenever a shim runs.
- Project requirements override globally active runtime, tool, and plugin versions.
- Managed tool entry points run through the selected Node.js runtime.
- Plugin tools execute in an isolated WASM sandbox managed by Wasmtime.
- No shell hook is required for command switching after the shims directory is on `PATH`.
- `jolter setup` refreshes shims and prints shell-specific `PATH` commands.
- Automatic setup shell detection supports PowerShell, Bash, Zsh, and Fish.

### Terminal Output

Status: **Complete**

- Distinct `[jolter]`, `[done]`, `[warn]`, and `[fail]` output vocabulary.
- Single-line interactive progress for runtime, tool, and plugin downloads.
- Current resolve, connect, fetch, verify, unpack, install, activate, remove, clean, configure, doctor, and shim actions.
- Download bytes, known-length percentage, and transfer rate.
- Stable line-oriented fallback for CI, redirected streams, and `--no-progress`.
- Global `--no-progress`, `--no-color`, `--quiet`, and `--verbose` controls.
- Automatic progress animation suppression in recognized CI environments.
- Width-aware `list` tables with aligned status and installation path columns.

### Storage

Status: **Complete for current artifacts**

```text
~/.jolter/
|-- runtimes/
|   |-- node/
|   |-- bun/
|   `-- deno/
|-- tools/
|   |-- npm/
|   |-- pnpm/
|   `-- yarn/
|-- plugins/
|   `-- @scope/
|       `-- plugin-name/
|-- plugin-tools/
|   `-- @scope/
|       `-- plugin-name/
|           `-- tool-name/
|-- shims/
|-- cache/
`-- config/
```

`JOLTER_HOME` can override the default root.

### Diagnostics

Status: **Complete**

Implemented checks:

- storage location
- project configuration resolution (Schema v1 and v2)
- matching runtime installation
- matching managed tool installation
- WASM plugin installation, manifest identity, and component integrity
- installed shim set
- shims directory presence on `PATH`
- machine-readable JSON output with overall health
- bounded runtime, tool, and plugin `--version` execution
- unrecognized cache entries
- conflicting Node/version-manager executables earlier on `PATH`
- unsupported CPU and operating system guidance
- writable storage and executable permissions
- Node/tool engine compatibility
- proxy and certificate environment diagnostics
- offline cache readiness
- actionable remediation in human-readable and JSON output

### Testing

Status: **Complete**

Implemented:

- unit tests across config, runtime selectors, resolver, storage, installer, WASM plugin runtime (`jolter-plugin`), shims, and orchestration
- CLI integration tests for runtimes, tools, and plugins
- setup, managed inventory, and JSON diagnostics integration tests
- synthetic verified ZIP and tarball installation tests
- WASM component execution and WIT interface validation tests
- corruption repair tests
- project-aware shim tests
- live npm registry and plugin registry smoke validation
- strict Clippy validation with warnings denied
- Windows x64, Linux x64, macOS x64, macOS ARM64, and WSL CI definitions
- measured test suite across 10 Rust crates
- CI enforcement of the 80% line-coverage target
- dependency vulnerability auditing
- tagged release archives and SHA-256 files

## Specification Coverage

| Specification area      | Status                          | Notes                                                                                                                                      |
| ----------------------- | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Rust modular workspace  | Complete                        | Ten responsibility-focused crates (`cli`, `config`, `core`, `doctor`, `installer`, `plugin`, `resolver`, `runtime`, `shim`, `storage`).    |
| Node.js management      | Complete                        | Install, select, activate, sync, repair, and shim dispatch work.                                                                           |
| Bun management          | Complete                        | Install, select, activate, sync, repair, and shim dispatch work.                                                                           |
| Deno management         | Complete                        | Install, select, activate, sync, repair, and shim dispatch work.                                                                           |
| Tool management         | Complete for built-in providers | npm, pnpm, and Yarn are installed, globally activated, project-selected, and dispatched with engine and optional Corepack hash validation. |
| Project pinning         | Complete                        | Runtime, tool, and plugin requirements can be pinned independently.                                                                        |
| Automatic switching     | Complete for commands           | Self-shims resolve the project on every invocation.                                                                                        |
| Reproducible sync       | Complete for current schema     | Runtime, tool, and plugin selectors are synchronized (`--yes` flag auto-installs plugins).                                                 |
| Doctor                  | Complete for stable core        | Version probes, manifests, engine checks, WASM component checks, conflicts, permissions, and remediation are implemented.                  |
| Repair                  | Complete for current artifacts  | Incomplete runtime, tool, and plugin directories are replaced safely.                                                                      |
| Security requirements   | Complete for current providers  | HTTPS, SHA-256/SRI integrity checks, WASM sandboxing, bounded extraction, and no telemetry.                                               |
| Telemetry               | Complete by absence             | No telemetry is collected. Opt-in telemetry is not implemented.                                                                            |
| Cross-platform support  | Complete                        | Native and WSL workflows pass on supported platform matrix.                                                                               |
| 80% coverage target     | Complete                        | Measured line coverage exceeds target, enforced by CI.                                                                                     |
| Extended tool providers | Complete                        | Custom tool provision fully supported via WASM plugins.                                                                                    |
| CI setup command        | Complete                        | Synchronizes runtimes, tools, and plugins; emits provider-neutral JSON plus GitHub Actions outputs.                                         |
| Plugin system           | Complete                        | WASM Component Model runtime (`wasmtime`), `@jolter/jdt` toolkit, `registry-server`, `registry-web`, `register-release-action`.           |

## Release Roadmap

### Milestone 0: Repository Foundation

Status: **Complete**

### Milestone 1: Runtime Lifecycle

Status: **Complete**

### Milestone 2: Project Switching and Tools

Status: **Complete**

### Milestone 3: Developer Experience and Diagnostics

Status: **Complete**

### Milestone 4: Cross-Platform Release Engineering

Status: **Complete**

### Milestone 5: Stable Core & Ecosystem Release

Status: **In release-candidate validation**

Delivered:

1. Configuration schema version 2 (`$schema: https://schemas.jolter.dev/project/v2/schema.json`) with `plugins` support and legacy defaulting.
2. Storage, configuration, and WASM plugin compatibility policy.
3. Plugin management subcommands (`jolter plugin install`, `list`, `update`, `remove`).
4. Uninstall, prune, and cache cleanup commands with active toolchain protection.
5. Installation, CI, WASM plugin development, registry operations, and troubleshooting documentation.
6. Threat-model documentation for WASM sandboxing and locking.

Remaining:

1. Finalize release-candidate period with real multi-project workloads.
2. Confirm all hosted release and provider-smoke jobs.

Target release: `1.0.0`

### Milestone 6: CI Automation

Status: **Complete**

Delivered scope:

- `jolter setup-ci` command supporting `--yes` auto-installation of missing project plugins.
- Detects GitHub Actions, GitLab CI, CircleCI, Azure Pipelines, Buildkite, and generic environments.
- Installs and activates exact project toolchains.
- Produces machine-readable resolved versions and cache/shim paths.
- Emits GitHub Actions path and step outputs.

### Milestone 7: Extended Tool Providers

Status: **Complete via WASM Plugins**

Delivered scope:

- Schema Version 2 support for custom plugin tools.
- Decoupled tool resolution from built-in binary implementations.
- WASM component guest execution targeting the `jolter:plugin` WIT interface.

### Milestone 8: Plugin System & Ecosystem

Status: **Complete**

Delivered scope:

- **`jolter-plugin` crate**: Embedded Wasmtime 36 component engine executing sandboxed WASM plugins.
- **`@jolter/jdt` (Jolter Development Toolkit)**: TypeScript/JS toolkit for initializing, building (`jco componentize`), running, validating, and packaging WASM plugins (`jdt init`, `build`, `run`, `validate`, `pack`).
- **`registry-server`**: PostgreSQL + Drizzle ORM Bun REST API server supporting GitHub OAuth, GitHub App organization access, package CRUD, RBAC, WebP logo optimization, SMTP notifications, passkey step-up, and release verification.
- **`registry-web`**: React management dashboard for plugin catalog browsing, account management, organization teams, alias requests, and registry administration.
- **`register-release-action`**: Official GitHub Action to publish verified releases from GitHub Actions to the Jolter Registry.
- **`jolter-plugin-jolter`**: Official reference plugin.

## Immediate Backlog

Highest priority:

1. Complete the beta validation period across real multi-project WASM plugin workflows.
2. Maintain scheduled smoke installations against Node.js, Bun, Deno, npm, and official WASM plugins.
3. Conduct external security audits for the Wasmtime sandbox and registry API endpoints.

Medium priority:

1. Machine-readable JSON output for mutating plugin lifecycle commands.
2. Extended fuzzing for WASM manifests, WIT interfaces, selectors, JSON, checksums, and archive paths.
3. Live network and proxy connectivity diagnostic probes.

Post-1.0 Future Scope:

1. **Registry Federation**: Support for self-hosted private enterprise registries in `jolter.json`.
2. **WASM Native Pre-compilation**: AOT compilation and Cranelift artifact caching for instant plugin initialization.
3. **Polyglot Plugin SDKs**: Native WIT binding generators for Rust, Go, and Zig plugin developers in addition to `@jolter/jdt` (JS/TS).
4. **Opt-in Anonymous Telemetry**: Strictly optional, privacy-preserving usage metrics if requested by the community.

## Release Risks

### WASM Host & Sandbox Security

Plugin code runs inside Wasmtime WASM component sandboxes. Host boundaries must remain strictly non-bypassable and memory-bounded.

### Provider & Registry Schema Changes

Jolter depends on Node.js release metadata, GitHub release assets, npm registry metadata, and Jolter Registry API endpoints. Schema changes fail closed and produce actionable diagnostics.

### Tool Compatibility & Engine Boundaries

Plugin-provided tools declare Node.js engine ranges and execution constraints. Mismatches are validated before execution to prevent broken runtime states.

## Definition of Stable

Jolter reaches stable `1.0.0` when:

- Node.js, Bun, Deno, and WASM plugin workflows pass on every supported platform.
- npm, pnpm, Yarn, and custom plugin tools install and dispatch reproducibly.
- All supported downloads and WASM binaries are authenticated by SHA-256/SRI integrity metadata.
- Project switching is reliable in documented shells.
- `doctor` detects and explains broken environments, invalid plugin manifests, and WASM host errors.
- `sync` and `repair` are safe under interruption and concurrency.
- Automated test coverage meets or exceeds 80%.
- No telemetry is enabled by default.
