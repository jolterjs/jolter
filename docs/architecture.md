# Architecture

Jolter is a Rust workspace split by responsibility:

- `cli`: command parsing, shell onboarding, output, and self-shim dispatch
- `core`: orchestration for use, pin, sync, repair, removal, pruning, and diagnostics
- `config`: strict `jolter.json` parsing and atomic writes
- `runtime`: runtime names and version-selector semantics
- `installer`: release providers, HTTPS downloads, checksums, and extraction
- `resolver`: project requirement discovery and source priority
- `shim`: shim generation and command-to-installation resolution
- `doctor`: non-destructive environment diagnostics
- `storage`: global layout, installed versions, and active selections

## Output Flow

Core and installer operations emit structured progress events instead of
writing directly to a terminal. Events cover selection, resolution, cache
reuse, connection, byte transfer, verification, extraction, publication,
activation, removal, cleanup, diagnostics, configuration, and shim refresh.

The CLI renderer owns presentation:

- interactive terminals redraw one compact status line on standard error;
- CI and non-terminal streams receive deterministic line-oriented events;
- ANSI styling is limited to terminal hosts that advertise color support,
  including an automatic plain-text fallback for legacy Windows consoles;
- structured list rows are width-aligned by the renderer without tab stops;
- JSON and completion commands suppress decorative events;
- final command results remain on standard output.

This separation keeps installer behavior testable and allows future output
formats without coupling provider code to terminal escape sequences.

## Runtime Flow

1. Resolve a selector against official release metadata.
2. Select the current operating-system, architecture, and CPU artifact.
3. Fetch the expected SHA-256 from official metadata.
4. Download to a temporary file inside the Jolter cache.
5. Verify the complete archive.
6. Extract into a temporary directory with path and size checks.
7. Verify the expected executable and write an installation manifest.
8. Rename the staged directory to its final version path.
9. Persist the active exact version and regenerate self-shims.

## Package Manager Flow

1. Resolve npm, pnpm, or Yarn from `jolter use`, `jolter.json`, or
   `package.json#packageManager`.
2. Query abbreviated metadata from the official npm registry.
3. Select the highest stable release matching the requested selector.
4. Validate the release's npm-style `engines.node` range against the selected
   project Node.js version.
5. Download the published tarball and verify its SHA-512 SRI value.
6. When an exact Corepack descriptor includes a hash, verify that hash over the
   same tarball.
7. Extract and publish it atomically under
   `~/.jolter/tools/<manager>/<version>`.
8. Persist the exact active version for manual `use` and project sync flows.
9. Dispatch its bundled JavaScript entry point through the selected Node.js
   runtime.

Locally reusable package-manager installations are checked against the
selected Node.js version using their installed `package.json` metadata.

Transient provider failures are retried at most three times with bounded
backoff. HTTP 408, 429, 500, 502, 503, and 504 responses are retryable.
Metadata cache publication is serialized by cache-key locks, while
installation publication is serialized by exact-version locks.

## Switching

Each generated shim is the Jolter executable under a tool-specific file name.
At startup, the CLI checks its invocation name before parsing commands.

The shim resolves project requirements and chooses the highest complete local
installation that matches. Project runtime and package manager requirements
take priority. If the project has no matching requirement, the shim uses the
exact globally active runtime or package manager version.

## Diagnostics and Inventory

`jolter list` reads storage directories without hiding incomplete
installations. It reports runtimes and managed package managers as `ready` only
when their expected executable or entry point exists.

`jolter doctor --json` serializes the same checks and health result used by the
human-readable command. This keeps automation and interactive diagnostics on
one behavior path.

## Storage Lifecycle

Exact-version uninstall and prune operations acquire the same installation
locks as install and repair. Active runtimes and package managers are protected
unless uninstall is explicitly forced. Prune protects active versions,
versions required by the current project, and the configured number of newest
complete versions.

Cache cleaning acquires an exclusive maintenance lock. Installation and repair
hold a shared maintenance lock so verified archives cannot disappear while an
operation is using them.

## Shell Onboarding

`jolter setup` refreshes the self-shims and emits commands for PowerShell,
Command Prompt, Bash, Zsh, or Fish. Shell profile and persistent environment
changes remain explicit user actions; the setup command does not rewrite
profile files itself.
