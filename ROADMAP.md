# Jolter Roadmap

Last updated: June 13, 2026

This roadmap translates the goals in [`spec.md`](spec.md) into an implementation
plan and records the repository's current development state. It is intended to
be the source of truth for project status, sequencing, and release readiness.

## Project Position

Current version: `0.2.0`

Current maturity: **beta / release-candidate preparation**

Current phase: **stable-core implementation complete; hosted release
validation is in progress**

Jolter can currently resolve, securely install, activate, pin, synchronize,
repair, and dispatch JavaScript runtimes. It can also manually activate or
project-pin npm, pnpm, and Yarn versions. The main architecture described by
the specification exists as a Rust workspace and the essential end-to-end
workflow is functional.

Jolter now includes the stable-core command surface, multiple pinned tools,
active-version updates, deeper diagnostics, storage lifecycle commands, CI
setup, release workflows, and measured coverage above the specification
target. It remains pre-1.0 until the configured native, WSL, provider-smoke,
audit, and release workflows have passed through a release-candidate period.
Additional tool providers and plugins remain explicitly post-stable work.

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
- Installation manifests for runtimes and managed tools.

### Tool Management

Status: **Complete for npm, pnpm, and Yarn alpha workflows**

- `jolter.json` tool requirements.
- `package.json#packageManager` discovery.
- npm installation from the official npm registry package.
- pnpm installation from the official npm registry package.
- Yarn installation from the official `@yarnpkg/cli-dist` package.
- Numeric, wildcard, and `latest` selectors.
- Exact Corepack-style descriptors with SHA-1 and SHA-2 hashes.
- Highest matching stable version selection.
- npm-style Node.js engine compatibility validation before installation and
  local reuse.
- Verified, cached, atomic installation under `~/.jolter/tools/`.
- Project-aware dispatch through the `npm`, `npx`, `pnpm`, and `yarn` shims.
- Manual installation and global activation through `jolter use`.
- Exact active tool persistence and inventory markers.
- Multi-tool synchronization and repair.
- Doctor checks for matching managed installations.

Known limitations:

- npm registry selection is currently fixed to the public official registry.
- Tool version execution is probed by `doctor`; Jolter also validates
  the exact managed installation selected from verified metadata.
- Bun's bundled package manager is available through the Bun runtime, but it is not a
  separately versioned managed tool.

### Project Configuration

Status: **Complete for the current schema**

- Strict `jolter.json` parsing with unknown-field rejection.
- Atomic configuration writes.
- One runtime requirement per project.
- One runtime and multiple tool requirements per project.
- Runtime and tool pinning through `jolter pin`.
- Pinning preserves unrelated runtime and tool requirements.
- Legacy `packageManager` input compatibility with canonical `tools` output.
- Configuration discovery from nested project directories.

Current schema:

```json
{
  "schemaVersion": 1,
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x",
    "yarn": "4.x"
  }
}
```

### Requirement Resolution

Status: **Complete for specified alpha sources**

Runtime priority:

1. `jolter.json`
2. `.node-version`
3. `.nvmrc`

Tool priority:

1. `jolter.json`
2. `package.json#packageManager`

Runtime and tool requirements are resolved independently, allowing a
project to combine a Node version file with a tool declaration in
`package.json`.

### Commands

| Command | Status | Current behavior |
| --- | --- | --- |
| `jolter use <tool>` | Complete | Installs or reuses a runtime or tool, activates it, and installs shims. Tools require active Node.js. |
| `jolter pin <runtime-or-tool>` | Complete | Writes a runtime or tool requirement to `jolter.json` while preserving other entries. |
| `jolter update [target]` | Complete | Updates an active runtime or tool within its major line, accepts explicit selectors, and supports `--all`. |
| `jolter setup` | Complete | Installs shims and prints exact current-session and persistent `PATH` commands for PowerShell, Command Prompt, Bash, Zsh, and Fish. |
| `jolter list` | Complete for current artifacts | Lists runtimes and managed tools, active markers, paths, and readiness. |
| `jolter doctor` | Complete for stable core | Probes versions, validates manifests and compatibility, detects conflicts, and emits remediation in text or JSON. |
| `jolter repair` | Complete for current artifacts | Reinstalls incomplete runtime and tool installations. |
| `jolter sync` | Complete for current schema | Makes the runtime and tool match project requirements. |
| `jolter uninstall <tool@version>` | Complete | Removes exact versions with active-toolchain protection and shared installer locks. |
| `jolter prune` | Complete | Preserves active, project-required, and configured newest versions; supports dry runs. |
| `jolter cache status/clean` | Complete | Inspects and safely cleans metadata and artifact caches. |
| `jolter setup-ci` | Complete | Synchronizes exact versions, reports cache/shim paths, emits JSON, and integrates with GitHub Actions outputs. |
| `jolter completions` | Complete | Generates Bash, Zsh, Fish, Elvish, and PowerShell completions. |

### Automatic Switching and Shims

Status: **Complete for invocation-time switching**

- Self-shims for `node`, `npm`, `npx`, `pnpm`, `yarn`, `bun`, and `deno`.
- Runtime selection is evaluated whenever a shim runs.
- Project requirements override globally active runtime and tool
  versions.
- Managed tool entry points run through the selected Node.js
  runtime.
- No shell hook is required for command switching after the shims directory is
  on `PATH`.
- `jolter setup` refreshes shims and prints shell-specific `PATH` commands.
- Automatic setup shell detection supports PowerShell, Bash, Zsh, and Fish.
- Explicit setup guidance is available for Command Prompt.

Release validation:

- execute the configured shell and WSL jobs on GitHub-hosted runners;
- observe real user environments with multiple version managers installed.

### Terminal Output

Status: **Complete for stable-core commands**

- Distinct `[jolter]`, `[done]`, `[warn]`, and `[fail]` output vocabulary.
- Single-line interactive progress for runtime and tool downloads.
- Current resolve, connect, fetch, verify, unpack, install, activate, remove,
  clean, configure, doctor, and shim actions.
- Download bytes, known-length percentage, and transfer rate.
- Stable line-oriented fallback for CI, redirected streams, and
  `--no-progress`.
- Global `--no-progress`, `--no-color`, `--quiet`, and `--verbose` controls.
- Automatic progress animation suppression in recognized CI environments.
- `NO_COLOR`, `JOLTER_NO_COLOR`, and `JOLTER_NO_PROGRESS` support.
- Conservative Windows ANSI capability detection with automatic plain-text
  fallback for legacy console hosts.
- Width-aware `list` tables with aligned status and installation path columns.
- Progress and operational logs use standard error; final results and
  machine-readable documents remain on standard output.

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

Status: **Complete for non-network stable-core diagnostics**

Implemented checks:

- storage location
- project configuration resolution
- matching runtime installation
- matching managed tool installation
- installed shim set
- shims directory presence on `PATH`
- machine-readable JSON output with overall health
- bounded runtime and tool `--version` execution
- installation manifest identity and integrity metadata
- unrecognized cache entries
- conflicting Node/version-manager executables earlier on `PATH`
- unsupported CPU and operating system guidance
- writable storage and executable permissions
- Node/tool engine compatibility
- proxy and certificate environment diagnostics
- offline cache readiness
- actionable remediation in human-readable and JSON output

Future depth:

- optional live proxy/TLS connectivity probes
- full installed-file content attestation beyond archive manifests

### Testing

Status: **Complete for repository release gates; hosted runs pending**

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
- Windows x64, Linux x64, macOS x64, macOS ARM64, and WSL CI definitions
- scheduled real-provider smoke installations
- concurrent same-version installation regression coverage
- bounded provider retry tests
- progress event, byte count, cache reuse, and CLI output-mode tests
- measured line coverage of 81.95%
- CI enforcement of the 80% line-coverage target
- dependency vulnerability auditing
- tagged release archives and SHA-256 files

Remaining research:

- proxy/corporate-certificate integration infrastructure
- selector, JSON, checksum, and archive-path fuzzing

## Specification Coverage

| Specification area | Status | Notes |
| --- | --- | --- |
| Rust modular workspace | Complete | Nine responsibility-focused crates are present. |
| Node.js management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Bun management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Deno management | Complete | Install, select, activate, sync, repair, and shim dispatch work. |
| Tool management | Complete for built-in providers | npm, pnpm, and Yarn are installed, globally activated, project-selected, and dispatched with engine and optional Corepack hash validation. |
| Project pinning | Complete | Runtime and multiple tool requirements can be pinned independently. |
| Automatic switching | Complete for commands | Self-shims resolve the project on every invocation. |
| Reproducible sync | Complete for current schema | Runtime and tool selectors are synchronized. |
| Doctor | Complete for stable core | Version probes, manifests, engine checks, conflicts, cache, permissions, environment, and remediation are implemented. |
| Repair | Complete for current artifacts | Incomplete runtime and tool directories are replaced safely. |
| Security requirements | Complete for current providers | HTTPS, integrity checks, bounded extraction, and no telemetry. |
| Telemetry | Complete by absence | No telemetry is collected. Opt-in telemetry is not implemented. |
| Cross-platform support | Release validation | Native and WSL workflows are configured; passing hosted runs are required before 1.0. |
| 80% coverage target | Complete | Measured line coverage is 81.95% and CI fails below 80%. |
| Extended tool providers | Future | The `tools` manifest is implemented; providers beyond npm, pnpm, and Yarn remain future work. |
| CI setup command | Complete | Synchronizes runtimes and tools and emits provider-neutral JSON plus GitHub Actions outputs. |
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

### Milestone 2: Project Switching and Tools

Status: **Complete for alpha**

Delivered:

- project-aware self-shims.
- runtime source priority.
- tool source priority.
- managed npm, pnpm, and Yarn installation.
- manual tool activation through `jolter use`.
- SHA-512 npm package integrity.
- Node-mediated tool dispatch.
- tool sync, repair, and doctor reporting.

Exit criteria met:

- a project can declare Node and pnpm/Yarn/npm requirements.
- `jolter sync` installs both layers.
- running a generated tool shim selects the project Node runtime and
  the matching managed tool.
- outside a project requirement, a generated tool shim selects the
  exact globally active managed version.
- all downloads are integrity checked before publication.

### Milestone 3: Developer Experience and Diagnostics

Status: **Complete**

Delivered:

- `jolter setup` with automatic or explicit shell selection.
- exact current-session and persistent `PATH` guidance.
- runtime and managed tool health in `jolter list`.
- machine-readable `jolter doctor --json` output.
- npm-style tool Node.js engine compatibility validation.
- exact Corepack-style `packageManager` hash parsing and archive verification.

Additional delivery:

- runtime and tool version probing with bounded execution;
- nvm, fnm, Volta, Corepack, and system executable precedence checks;
- remediation actions in text and JSON findings;
- offline cache and provider failure guidance;
- interactive progress rendering and deterministic CI logging.

Exit criteria:

- a new user can install Jolter and configure `PATH` without manual path
  discovery.
- doctor identifies common conflicts and gives an actionable repair command.
- list output covers both runtimes and managed tools.
- tool declarations commonly generated by Corepack-compatible tools
  are accepted.

### Milestone 4: Cross-Platform Release Engineering

Status: **Implemented; hosted validation pending**

Delivered:

1. CI for Windows x64, Linux x64, macOS x64, and macOS ARM64.
2. WSL validation workflow.
3. Tagged release builds and SHA-256 files.
4. Scheduled smoke installations against Node.js, Bun, Deno, and npm.
5. Coverage collection with an 80% line threshold.
6. Rust dependency vulnerability auditing.
7. Installation documentation and release packaging.

Remaining:

1. Observe successful hosted runs on all configured runners.
2. Add dedicated proxy, corporate-certificate, and interrupted-download
   integration infrastructure.

Exit criteria:

- every supported platform passes unit, integration, lint, and provider smoke
  tests.
- release artifacts are reproducible enough to audit and are published with
  checksums.
- coverage is measured and remains at or above the specification target.

### Milestone 5: Stable Core Release

Status: **In release-candidate validation**

Delivered:

1. Configuration schema version 1 with legacy defaulting and fail-closed
   unknown-version handling.
2. Storage and configuration compatibility policy.
3. Uninstall, prune, and cache cleanup commands.
4. Installation, CI, lifecycle, compatibility, diagnostics, and
   troubleshooting documentation.
5. Updated locking and doctor execution threat-model documentation.

Remaining:

1. Complete an external or independent security review.
2. Run the beta period with real multi-project workflows.
3. Confirm all hosted release and provider-smoke jobs.

Exit criteria:

- no known data-loss or unsafe extraction defects.
- configuration and shim behavior have stable compatibility guarantees.
- upgrade, repair, and removal paths are documented and tested.
- the core Node/Bun/Deno plus npm/pnpm/Yarn workflows are reliable on every
  supported platform.

Target release: `1.0.0`

### Milestone 6: CI Automation

Status: **Complete for the initial provider-neutral command**

Planned command:

```bash
jolter setup-ci
```

Delivered scope:

- detects GitHub Actions, GitLab CI, CircleCI, Azure Pipelines, Buildkite, and
  generic environments;
- installs and activates exact project toolchains;
- produces machine-readable resolved versions and cache/shim paths;
- emits GitHub Actions path and step outputs;
- supports offline or prewarmed caches through the normal installer;
- keeps provider integration in the CLI rather than core APIs.

### Milestone 7: Extended Tool Providers

Status: **Manifest complete; providers future**

Implemented schema:

```json
{
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x",
    "yarn": "4.x"
  }
}
```

Remaining provider requirements:

- preserve input compatibility with legacy `packageManager`.
- distinguish executable tools from project dependencies.
- define verified providers and entry points for each new tool.
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

1. Run and stabilize the new hosted native, WSL, audit, and coverage jobs.
2. Run and stabilize the scheduled real-provider smoke workflow.
3. Complete an independent security review.
4. Exercise release archives in beta multi-project workflows.
5. Add dedicated proxy and corporate-certificate integration tests.

Medium priority:

1. Additional machine-readable output for mutating lifecycle commands.
2. Fuzzing for selectors, JSON, checksums, and archive paths.
3. Optional live network diagnostics.

Later:

1. extended tool providers.
2. plugin system.
3. explicitly opt-in anonymous telemetry, only if the project decides it is
   useful and can document a privacy-preserving design.

## Release Risks

### Cross-Platform Confidence

Native and WSL workflows now encode the support matrix, but they must pass
repeatedly on hosted runners before the implementation is called stable.

### Provider Changes

Jolter depends on Node.js release metadata, GitHub release assets, and npm
registry metadata. Provider schema changes must fail closed and produce clear
diagnostics.

### Tool Compatibility

Tool `engines.node` requirements are validated before installation
and when reusing an installed tool. The remaining risk is incomplete or
incorrect upstream metadata, which should remain visible as a provider error
rather than producing a silently unusable toolchain.

### Configuration Compatibility

The current schema is small and strict. Schema versioning and migration must be
designed before stable releases add extended tools or plugins.

### Storage Lifecycle

Removal, pruning, cache cleanup, active-version protection, and shared
maintenance locking are implemented. The remaining risk is validation against
long-lived real-world storage accumulated across multiple pre-1.0 releases.

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
