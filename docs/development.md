# Development

This guide is for contributors working on the Jolter repository.

## Prerequisites

- Rust `1.85` or newer
- Cargo
- Git
- platform build tools required by the selected Rust target

Clone the repository and build the workspace:

```bash
cargo build --workspace --all-targets --locked
```

The development binary is:

```text
Windows: target/debug/jolter.exe
Unix:   target/debug/jolter
```

## Workspace Layout

| Crate       | Responsibility                                                                    |
| ----------- | --------------------------------------------------------------------------------- |
| `cli`       | Command parsing, terminal output, shell setup, CI integration, and shim dispatch. |
| `core`      | User-operation orchestration.                                                     |
| `config`    | `jolter.json` parsing, validation, discovery, and atomic writes.                  |
| `runtime`   | Runtime/tool identities and selector semantics.                                   |
| `installer` | Providers, networking, cache, integrity, extraction, and publication.             |
| `resolver`  | Project requirement discovery and precedence.                                     |
| `shim`      | Shim installation and command target resolution.                                  |
| `doctor`    | Non-destructive environment and installation checks.                              |
| `storage`   | Filesystem layout, inventory, activation, and statistics.                         |

Read [architecture](architecture.md) before changing behavior across crate
boundaries.

## Required Quality Gates

Format:

```bash
cargo fmt --all -- --check
```

Lint:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Test:

```bash
cargo test --workspace --all-targets --locked
```

Coverage:

```bash
cargo llvm-cov \
  --workspace \
  --all-targets \
  --locked \
  --fail-under-lines 80
```

Dependency audit:

```bash
cargo audit
```

CI runs formatting, Clippy, native tests on Windows/Linux/macOS, WSL tests,
coverage enforcement, and dependency auditing.

## Testing Strategy

- Unit tests cover selector parsing, configuration validation, storage,
  providers, rendering, and orchestration.
- CLI integration tests run the compiled command with isolated temporary
  projects and `JOLTER_HOME`.
- Provider code uses controlled clients and fixtures for deterministic
  integrity and failure testing.
- The scheduled provider-smoke workflow exercises real Node.js, Bun, Deno,
  and pnpm providers on supported runners.

Tests must not depend on a contributor's real `~/.jolter`. Set `JOLTER_HOME`
to a temporary directory for manual experiments:

```bash
export JOLTER_HOME="$(mktemp -d)"
cargo run -p jolter-cli -- use node@24
```

PowerShell:

```powershell
$env:JOLTER_HOME = Join-Path $env:TEMP "jolter-dev"
cargo run -p jolter-cli -- use node@24
```

## Change Expectations

Behavioral changes should update:

1. implementation and focused tests;
2. CLI help text when the command surface changes;
3. user documentation;
4. `spec.md` when the contract changes;
5. `ROADMAP.md` when delivery status changes;
6. `CHANGELOG.md` for user-visible release notes.

Preserve legacy input compatibility unless an intentional migration is
designed and documented.

## Security-Sensitive Areas

Changes to URL validation, redirects, checksums, archive extraction, symlink
handling, permissions, locking, temporary paths, or atomic publication require
adversarial tests. Read [security](security.md) before modifying installer or
storage behavior.

## Documentation Standards

Commands in user guides must match `jolter --help`. Distinguish current
behavior from roadmap work. Use relative Markdown links for repository docs
and avoid documenting installer flags that the public scripts do not support.
