# Architecture

Jolter is a native Rust CLI with no resident daemon. It combines project
requirement resolution, verified installation, filesystem-backed activation,
and command shims in one workspace.

## Design Principles

- Project declarations are portable and belong in source control.
- Installed toolchains are rebuildable machine state.
- Network artifacts remain untrusted until verified.
- Publication is atomic and exact-version operations are lock-coordinated.
- Shims resolve on command invocation; directory changes need no shell hook.
- Human output and machine output share behavior but not presentation.
- Diagnostics are non-destructive and provide remediation where possible.

## Workspace Boundaries

| Crate | Owns |
| --- | --- |
| `cli` | Argument parsing, output rendering, shell instructions, CI adaptation, completions, and process launch. |
| `core` | Orchestration for use, pin, update, sync, repair, lifecycle, and diagnostics. |
| `config` | Strict project schema, upward discovery, validation, and atomic writes. |
| `runtime` | Runtime/tool identifiers, request parsing, selectors, and hashes. |
| `installer` | Provider metadata, HTTP, retry, cache, integrity, extraction, locking, and publication. |
| `resolver` | Project-source precedence and normalized requirements. |
| `shim` | Shim generation and command-to-installation selection. |
| `doctor` | Storage, project, installation, command-routing, network, and cache checks. |
| `storage` | Directory layout, inventory, active selections, and filesystem statistics. |

Dependencies point toward domain types and storage rather than terminal
presentation. Installer and core operations emit progress events through a
reporter interface instead of printing directly.

## Command Flow

A normal CLI operation follows:

1. parse global output controls and the subcommand;
2. discover `JOLTER_HOME` and create the base layout;
3. construct installer and orchestration services;
4. resolve project or direct-request inputs;
5. perform installation, activation, or inspection;
6. refresh shims when the operation changes executable routing;
7. render a final result or machine payload.

Errors cross crate boundaries as typed errors and become one user-facing
failure at the CLI edge.

Individual exact-version publication is atomic. Multi-target orchestration is
restartable rather than globally transactional: if a later runtime, tool, or
removal fails, previously completed operations remain valid and are reused on
the next run.

## Project Resolution

The resolver canonicalizes the starting directory, searches upward, and
normalizes requirements into:

- an optional runtime request with its source;
- zero or more tool requests with their source;
- a project root.

Runtime precedence:

1. `jolter.json`;
2. `.node-version`;
3. `.nvmrc`.

Tool precedence:

1. a non-empty `jolter.json#tools`;
2. `package.json#packageManager`.

Global activation is a shim fallback, not a project requirement, so `sync`
cannot accidentally treat a developer's default as repository configuration.

## Runtime Installation

1. Parse and validate the request.
2. Query or reuse provider metadata.
3. select the highest stable matching release.
4. Select the operating-system and architecture artifact.
5. Obtain trusted SHA-256 metadata.
6. Acquire cache and exact-version coordination as needed.
7. Download into a temporary cache file.
8. Verify the complete archive.
9. Extract into a temporary directory with path and size controls.
10. Verify the expected executable.
11. Write `.jolter-install.json`.
12. Rename the staged directory to its final exact-version path.
13. Persist activation when requested.

Exact complete installations can be reused. Broad selectors that describe a
moving provider state require suitable metadata.

## Managed Tool Installation

1. Resolve npm, pnpm, or Yarn from a direct request or project source.
2. Require an exact selected Node.js runtime.
3. Query abbreviated package metadata from the npm registry.
4. Select the highest stable matching release.
5. Validate `engines.node`.
6. Download the package tarball.
7. Verify registry SHA-512 SRI.
8. Verify an optional exact Corepack-style hash.
9. Extract into a temporary tool directory.
10. Verify the expected JavaScript entry point.
11. Write `.jolter-tool.json`.
12. Atomically publish under `tools/<kind>/<version>`.
13. Dispatch the entry point through the selected Node.js executable.

Reusable tool installations are rechecked against the selected Node.js
version.

## Cache and Concurrency

The cache separates provider metadata, verified downloads, and locks.

- Metadata-key locks serialize publication of one metadata response.
- Runtime-version locks serialize exact runtime publication and removal.
- Tool-version locks serialize exact tool publication and removal.
- A maintenance lock coordinates cache cleaning with operations using cached
  artifacts.

Temporary files live on the same storage volume as their publication target
where atomic rename semantics are required.

## Activation

Global active versions are stored as exact semantic versions in
`config/active.json`. Runtime and tool kinds share this activation map.

Activation is not environment-variable mutation. The shim reads project
requirements and global state on each invocation. Forced uninstall clears an
activation only when it removes that exact active version.

## Shim Dispatch

Shims are copies of the Jolter executable under supported command names. At
startup, Jolter checks its invocation filename before normal CLI parsing.

Runtime dispatch:

1. resolve a matching project installation;
2. otherwise read the exact global activation;
3. verify the executable exists;
4. launch it with the original arguments and constructed environment.

Tool dispatch:

1. resolve the selected Node.js runtime;
2. resolve a project or global managed tool where applicable;
3. fall back to bundled npm/npx for Node.js where applicable;
4. launch the JavaScript entry point through Node.js.

Missing versions fail explicitly. Shim dispatch does not perform downloads.

## Output Architecture

Core and installer operations emit structured events for selection,
resolution, cache reuse, connection, transfer, verification, extraction,
publication, activation, cleanup, configuration, diagnostics, and shims.

The CLI renderer chooses:

- one updating line for interactive terminals;
- deterministic event lines for CI and redirected streams;
- quiet final-only presentation;
- silent decoration for JSON and generated scripts.

Final results go to standard output. Progress and failures go to standard
error. ANSI rendering is conditional on terminal capability and user controls.

## Diagnostics

Inventory reads storage directly and intentionally includes incomplete
semantic-version directories. Doctor combines:

- pure filesystem and configuration checks;
- project resolution;
- manifest and permission validation;
- bounded subprocess probes;
- `PATH` and competing-manager inspection;
- cache and network-environment checks.

Human and JSON doctor modes serialize the same report so automation cannot
silently diverge from interactive behavior.

## Lifecycle Operations

Uninstall and prune use installer removal primitives under exact-version
locks. Prune computes a protected set from:

- global activations;
- current project requirements;
- per-kind complete-version retention.

Cache cleanup uses exclusive maintenance coordination. It does not delete
installed versions or activation state.

## Compatibility Boundaries

Canonical schema-version-1 output uses `tools`. The config parser accepts the
legacy `packageManager` map alias, and diagnostics accept legacy tool manifest
fields. The resolver separately supports the standard string-valued
`package.json#packageManager` source.

See [compatibility](compatibility.md) before changing serialized fields,
storage paths, JSON output, or provider selection.

## Extension Points

New runtime or tool providers require coordinated changes across domain types,
installer providers, storage paths, shim commands, doctor checks, CLI parsing,
tests, and docs. A plugin system is roadmap work and is not part of the current
runtime extension model.
