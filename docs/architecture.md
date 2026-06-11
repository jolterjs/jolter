# Architecture

Jolter is a Rust workspace split by responsibility:

- `cli`: command parsing, output, and self-shim process dispatch
- `core`: orchestration for use, pin, sync, repair, and diagnostics
- `config`: strict `jolter.json` parsing and atomic writes
- `runtime`: runtime names and version-selector semantics
- `installer`: release providers, HTTPS downloads, checksums, and extraction
- `resolver`: project requirement discovery and source priority
- `shim`: shim generation and command-to-installation resolution
- `doctor`: non-destructive environment diagnostics
- `storage`: global layout, installed versions, and active selections

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

## Switching

Each generated shim is the Jolter executable under a tool-specific file name.
At startup, the CLI checks its invocation name before parsing commands.

The shim resolves project requirements and chooses the highest complete local
installation that matches. If the project has no matching requirement, it
uses the exact globally active version.

