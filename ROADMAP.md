# Jolter Roadmap

Last updated: June 12, 2026

This roadmap translates the goals in [`spec.md`](spec.md) into an implementation
plan and records the repository's current development state. It is intended to
be the source of truth for project status, sequencing, and release readiness.

## Project Position

Current version: `0.1.0`

Current maturity: **alpha**

Current phase: **core toolchain lifecycle implemented; developer experience and
production hardening are in progress**

Jolter can currently resolve, securely install, activate, pin, synchronize,
repair, and dispatch JavaScript runtimes. It can also install and dispatch
project-pinned npm, pnpm, and Yarn versions. The main architecture described by
the specification exists as a Rust workspace and the essential end-to-end
workflow is functional.

Jolter is not yet ready for a stable release. The largest remaining gaps are
cross-platform CI coverage, shell onboarding, richer diagnostics, package
manager compatibility checks, lifecycle commands such as uninstall, and the
future CI/plugin/tool manifest work described in the specification.

## Status Legend

- **Complete**: implemented and covered by automated tests.
- **Partial**: useful implementation exists, but one or more specified
  behaviors or release-quality requirements remain.
- **Planned**: accepted scope with no complete implementation yet.
- **Future**: intentionally deferred until after the stable core.

## Current Features

### Runtime Management

Status: **Complete for the alpha core**

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

Status: **Complete for current providers**

- HTTPS-only metadata and artifact requests.
- HTTPS-only redirect policy.
- SHA-256 verification for Node.js, Bun, and Deno artifacts.
- SHA-512 Subresource Integrity verification for npm registry packages.
- Bounded metadata, archive, entry-count, and extracted-size handling.
- Archive path traversal protection.
- Symlink and hard-link validation.
- Temporary staging followed by atomic publication.
- Cross-process installation locks.
- Verified artifact caching.
- Cached release metadata with offline fallback.
- `JOLTER_OFFLINE=1` support.
- No downloaded executable is run during installation.
- Installation manifests for runtimes and managed package managers.

### Package Manager Management

Status: **Complete for npm, pnpm, and Yarn alpha workflows**

- `jolter.json` package manager requirements.
- `package.json#packageManager` discovery.
- npm installation from the official npm registry package.
- pnpm installation from the official npm registry package.
- Yarn installation from the official `@yarnpkg/cli-dist` package.
- Numeric, wildcard, and `latest` selectors.
- Highest matching stable version selection.
- Verified, cached, atomic installation under `~/.jolter/tools/`.
- Project-aware dispatch through the `npm`, `npx`, `pnpm`, and `yarn` shims.
- Package manager synchronization and repair.
- Doctor checks for matching managed installations.

Known limitations:

- Corepack-style selectors with an appended hash are not parsed yet.
- Node.js engine compatibility is not validated before installing a package
  manager.
- npm registry selection is currently fixed to the public official registry.
- Package manager version execution is not probed by `doctor`; Jolter validates
  the exact managed installation selected from verified metadata.
- Bun's package manager is available through the Bun runtime, but it is not a
  separately versioned managed tool.

### Project Configuration

Status: **Complete for the current schema**

- Strict `jolter.json` parsing with unknown-field rejection.
- Atomic configuration writes.
- One runtime requirement per project.
- One package manager requirement per project.
- Runtime pinning through `jolter pin`.
- Existing package manager configuration is preserved when pinning a runtime.
- Configuration discovery from nested project directories.

Current schema:

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

### Requirement Resolution

Status: **Complete for specified alpha sources**

Runtime priority:

1. `jolter.json`
2. `.node-version`
3. `.nvmrc`

Package manager priority:

1. `jolter.json`
2. `package.json#packageManager`

Runtime and package manager requirements are resolved independently, allowing a
project to combine a Node version file with a package manager declaration in
`package.json`.

### Commands

| Command | Status | Current behavior |
| --- | --- | --- |
| `jolter use <runtime>` | Complete | Installs or reuses a runtime, activates it, and installs shims. |
| `jolter pin <runtime>` | Complete | Writes the runtime requirement to `jolter.json`. |
| `jolter setup` | Complete | Installs shims and prints exact current-session and persistent `PATH` commands for PowerShell, Command Prompt, Bash, Zsh, and Fish. |
| `jolter list` | Complete for current artifacts | Lists runtimes and managed package managers, active runtime markers, paths, and readiness. |
| `jolter doctor` | Partial | Checks config, runtime, package manager, shims, and `PATH`; supports human-readable and JSON output. |
| `jolter repair` | Complete for current artifacts | Reinstalls incomplete runtime and package manager installations. |
| `jolter sync` | Complete for current schema | Makes the runtime and package manager match project requirements. |
| `jolter setup-ci` | Planned | Not implemented. |

### Automatic Switching and Shims

Status: **Complete for invocation-time switching**

- Self-shims for `node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, and `deno`.
- Runtime selection is evaluated whenever a shim runs.
- Project requirements override globally active runtime versions.
- Managed package manager entry points run through the selected Node.js
  runtime.
- No shell hook is required for command switching after the shims directory is
  on `PATH`.
- `jolter setup` refreshes shims and prints shell-specific `PATH` commands.
- Automatic setup shell detection supports PowerShell, Bash, Zsh, and Fish.
- Explicit setup guidance is available for Command Prompt.

Remaining work:

- Detect conflicting version managers and shadowed shims.
- Add execution tests for each supported shell and WSL.

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
|-- shims/
|-- cache/
`-- config/
```

`JOLTER_HOME` can override the default root.

### Diagnostics

Status: **Partial**

Implemented checks:

- storage location
- project configuration resolution
- matching runtime installation
- matching managed package manager installation
- installed shim set
- shims directory presence on `PATH`
- machine-readable JSON output with overall health

Remaining checks:

- execute `--version` safely and compare the reported version
- installation manifest integrity
- corrupt or orphaned cache entries
- conflicting Node/version-manager executables earlier on `PATH`
- unsupported CPU and operating system guidance
- writable storage and executable permissions
- Node/package-manager engine compatibility
- proxy and TLS diagnostics
- offline cache readiness

### Testing

Status: **Partial toward release requirements**

Implemented:

- unit tests across config, runtime selectors, resolver, storage, installer,
  shims, and orchestration
- CLI integration tests
- setup, managed inventory, and JSON diagnostics integration tests
- synthetic verified ZIP and tarball installation tests
- corruption repair tests
- project-aware shim tests
- live npm registry smoke validation performed during development
- strict Clippy validation with warnings denied

Remaining:

- automated Windows, Linux, macOS, and WSL CI matrix
- real provider installation smoke tests in CI
- network failure and proxy integration tests
- concurrent process integration tests
- shell-specific shim execution tests
- measured coverage reporting and enforcement of the 80% target
- fuzzing for selectors, JSON, checksums, and archive paths

## Specification Coverage

| Specification area | Status | Notes |
| --- | --- | --- |
| Rust modular workspace | Complete | Nine responsibility-focused crates are present. |
| Node.js management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Bun management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Deno management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Package manager management | Complete for alpha | npm, pnpm, and Yarn are installed and dispatched as managed tools. |
| Project pinning | Complete | Runtime pinning is implemented. |
| Automatic switching | Complete for commands | Self-shims resolve the project on every invocation. |
| Reproducible sync | Complete for current schema | Runtime and package manager selectors are synchronized. |
| Doctor | Partial | Core checks and JSON output exist; deeper health checks remain. |
| Repair | Complete for current artifacts | Incomplete runtime and tool directories are replaced safely. |
| Security requirements | Complete for current providers | HTTPS, integrity checks, bounded extraction, and no telemetry. |
| Telemetry | Complete by absence | No telemetry is collected. Opt-in telemetry is not implemented. |
| Cross-platform support | Partial | Code paths exist; full CI validation is missing. |
| 80% coverage target | Planned measurement | Tests exist, but coverage is not measured in CI. |
| Extended tools manifest | Future | Deferred until after the package manager core stabilizes. |
| CI setup command | Planned | No implementation yet. |
| Plugin system | Future | Architecture boundaries help, but no public plugin API exists. |

## Release Roadmap

### Milestone 0: Repository Foundation

Status: **Complete**

Delivered:

- Rust workspace and crate boundaries.
- CLI command surface.
- strict configuration model.
- runtime selector model.
- storage layout.
- architecture and security documentation.
- test and lint baseline.

### Milestone 1: Runtime Lifecycle

Status: **Complete**

Delivered:

- official release providers for Node.js, Bun, and Deno.
- secure artifact resolution and installation.
- local version discovery and activation.
- `use`, `list`, `sync`, and `repair`.
- metadata and artifact caches.
- offline mode.

Exit criteria met:

- each supported runtime can be resolved and installed.
- selectors choose the highest matching stable release.
- corrupt partial installations are detected and repairable.
- installed runtimes are never published before verification completes.

### Milestone 2: Project Switching and Package Managers

Status: **Complete for alpha**

Delivered:

- project-aware self-shims.
- runtime source priority.
- package manager source priority.
- managed npm, pnpm, and Yarn installation.
- SHA-512 npm package integrity.
- Node-mediated package manager dispatch.
- package manager sync, repair, and doctor reporting.

Exit criteria met:

- a project can declare Node and pnpm/Yarn/npm requirements.
- `jolter sync` installs both layers.
- running a generated package manager shim selects the project Node runtime and
  the matching managed package manager.
- all downloads are integrity checked before publication.

### Milestone 3: Developer Experience and Diagnostics

Status: **In progress**

Delivered:

- `jolter setup` with automatic or explicit shell selection.
- exact current-session and persistent `PATH` guidance.
- runtime and managed package manager health in `jolter list`.
- machine-readable `jolter doctor --json` output.

Remaining priority work:

1. Add runtime and package manager version probing to `doctor`.
2. Detect conflicting `nvm`, `fnm`, Volta, Corepack, and system executable
   precedence.
3. Add clear remediation actions to doctor findings.
4. Validate package manager Node engine requirements before installation.
5. Support Corepack-style `packageManager` hashes.
6. Improve error messages for offline cache misses and provider failures.

Exit criteria:

- a new user can install Jolter and configure `PATH` without manual path
  discovery.
- doctor identifies common conflicts and gives an actionable repair command.
- list output covers both runtimes and managed tools.
- package manager declarations commonly generated by Corepack-compatible tools
  are accepted.

### Milestone 4: Cross-Platform Release Engineering

Status: **Planned**

Work:

1. Add CI for Windows x64, Linux x64, macOS x64, and macOS ARM64.
2. Add WSL validation.
3. Add release builds and checksums.
4. Add smoke installations against real Node.js, Bun, Deno, and npm registry
   providers.
5. Add code coverage collection and enforce the 80% target.
6. Add supply-chain auditing for Rust dependencies.
7. Test proxies, corporate certificates, and interrupted downloads.
8. Add installation documentation and release packaging.

Exit criteria:

- every supported platform passes unit, integration, lint, and provider smoke
  tests.
- release artifacts are reproducible enough to audit and are published with
  checksums.
- coverage is measured and remains at or above the specification target.

### Milestone 5: Stable Core Release

Status: **Planned**

Work:

1. Stabilize configuration and storage schemas.
2. Add migration rules for future schema changes.
3. Add uninstall/prune commands and cache cleanup.
4. Define compatibility and support policy.
5. Complete security review and threat-model update.
6. Publish complete command and troubleshooting documentation.
7. Run a beta period with real multi-project workflows.

Exit criteria:

- no known data-loss or unsafe extraction defects.
- configuration and shim behavior have stable compatibility guarantees.
- upgrade, repair, and removal paths are documented and tested.
- the core Node/Bun/Deno plus npm/pnpm/Yarn workflows are reliable on every
  supported platform.

Target release: `1.0.0`

### Milestone 6: CI Automation

Status: **Future after the stable core**

Planned command:

```bash
jolter setup-ci
```

Potential scope:

- detect common CI providers.
- emit or apply cache configuration.
- install and activate exact project toolchains.
- produce machine-readable resolved versions.
- support offline or prewarmed caches.
- avoid provider-specific lock-in in core APIs.

### Milestone 7: Extended Toolchain Manifest

Status: **Future**

Planned schema direction:

```json
{
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x",
    "typescript": "5.x"
  }
}
```

Design requirements:

- preserve compatibility with `packageManager`.
- distinguish executable tools from project dependencies.
- define verified providers and entry points.
- avoid turning Jolter into a package manager.
- support deterministic tool resolution and shims.

### Milestone 8: Plugin System

Status: **Future**

Potential extension points:

- runtime providers
- tool providers
- release metadata resolvers
- artifact verification strategies
- custom project requirement sources

Constraints:

- plugins must not weaken core HTTPS or integrity policies silently.
- plugin failures must be isolated and diagnosable.
- the stable core configuration must remain usable without plugins.
- plugin APIs must be versioned independently enough to avoid frequent core
  breaking changes.

## Immediate Backlog

Highest priority:

1. Package manager engine compatibility validation.
2. Corepack-style package manager hash parsing.
3. Runtime and package manager version probing.
4. Conflict detection and actionable doctor remediation.
5. Cross-platform GitHub Actions matrix.
6. Coverage reporting.

Medium priority:

1. `jolter uninstall` and `jolter prune`.
2. Cache inspection and cleanup.
3. Better progress output for downloads and extraction.
4. Provider retry policy with bounded backoff.
5. Config schema versioning and migration plan.
6. Shell completion generation.
7. Machine-readable command output.

Later:

1. `setup-ci`.
2. extended tool manifest.
3. plugin system.
4. explicitly opt-in anonymous telemetry, only if the project decides it is
   useful and can document a privacy-preserving design.

## Release Risks

### Cross-Platform Confidence

The code has explicit Windows, Linux, and macOS behavior, but the repository
does not yet prove all paths continuously. CI coverage is required before
calling the implementation broadly supported.

### Provider Changes

Jolter depends on Node.js release metadata, GitHub release assets, and npm
registry metadata. Provider schema changes must fail closed and produce clear
diagnostics.

### Package Manager Compatibility

A package manager version can require a newer Node.js version than the project
selected. This needs pre-install compatibility validation to prevent a
successfully synchronized but unusable toolchain.

### Configuration Compatibility

The current schema is small and strict. Schema versioning and migration must be
designed before stable releases add extended tools or plugins.

### Storage Lifecycle

Install and repair are present, but removal and pruning are not. Long-lived
users will accumulate versions and cache artifacts until lifecycle commands are
implemented.

## Definition of Stable

Jolter reaches stable `1.0.0` when:

- Node.js, Bun, and Deno workflows pass on every supported platform.
- npm, pnpm, and Yarn requirements install and dispatch reproducibly.
- all supported downloads are authenticated by expected integrity metadata.
- project switching is reliable in documented shells.
- doctor detects and explains the most common broken environments.
- sync and repair are safe under interruption and concurrency.
- configuration and storage compatibility rules are published.
- installation, upgrade, uninstall, and troubleshooting docs are complete.
- automated test coverage meets or exceeds 80%.
- no telemetry is enabled by default.
