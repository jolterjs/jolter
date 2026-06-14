# Release Operations

This runbook describes the repository's current tag-driven release process.
It is intended for maintainers and assumes permission to publish GitHub
releases and update the public installer endpoints.

## Supported Release Targets

The release workflow builds:

| Platform | Rust target | Archive |
| --- | --- | --- |
| Linux x64 | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| Windows x64 | `x86_64-pc-windows-msvc` | `.zip` |
| macOS x64 | `x86_64-apple-darwin` | `.tar.gz` |
| macOS ARM64 | `aarch64-apple-darwin` | `.tar.gz` |

Every archive is accompanied by a `.sha256` checksum file.

## Release Preconditions

Before creating a tag:

1. confirm the intended version in the workspace manifest;
2. finalize the matching `CHANGELOG.md` section with an absolute release date;
3. ensure `README.md`, `docs/`, `spec.md`, and `ROADMAP.md` describe the shipped
   command surface;
4. run all local quality gates from [development](development.md);
5. confirm the latest native, WSL, coverage, audit, and provider-smoke runs;
6. test install, setup, sync, doctor, update, uninstall, and prune on a clean
   temporary `JOLTER_HOME`;
7. verify legacy schema and manifest compatibility fixtures still pass.

Do not publish from a dirty or unreviewed worktree.

## Tagging

Releases are triggered by semantic version tags:

```bash
git tag -a v0.2.0 -m "Jolter v0.2.0"
git push origin v0.2.0
```

The pushed tag must match the workspace version. Pre-release tags should use a
valid semantic version suffix and must be tested against installer version
selection before publication.

## Automated Build

`.github/workflows/release.yml`:

1. checks out the tagged source;
2. builds each target with `cargo build --release --locked`;
3. packages the binary with `README.md` and `LICENSE`;
4. calculates SHA-256 files;
5. uploads per-target workflow artifacts;
6. combines them into a GitHub release with generated notes.

The workflow uses thin LTO, one codegen unit, and stripped symbols as
configured by the workspace release profile.

## Artifact Verification

After the workflow completes:

1. download every release archive and checksum;
2. verify the checksum on a separate machine or clean runner;
3. inspect archive paths and required files;
4. execute `jolter --version`;
5. run `jolter setup` with a temporary home;
6. install and execute at least one supported runtime;
7. verify the release notes and attached filenames.

Unix:

```bash
sha256sum -c jolter-v0.2.0-x86_64-unknown-linux-gnu.tar.gz.sha256
```

macOS:

```bash
shasum -a 256 -c jolter-v0.2.0-aarch64-apple-darwin.tar.gz.sha256
```

Windows PowerShell:

```powershell
Get-FileHash .\jolter-v0.2.0-x86_64-pc-windows-msvc.zip -Algorithm SHA256
```

Compare the PowerShell result with the sibling checksum file.

## Installer Publication

The public entry points are:

```text
https://jolter.dev/win/install
https://jolter.dev/unix/install
```

Before announcing a release, verify that both endpoints resolve, select the
intended release, download the matching platform artifact, validate its
checksum, and install a working binary.

Installer deployment is operationally separate from the tag workflow in this
repository. Record the deployed installer revision and release version in the
release evidence.

## Smoke Matrix

At minimum, validate:

- Windows PowerShell installation and shell setup;
- Linux installation and Bash setup;
- macOS installation on every published architecture;
- WSL setup and command dispatch;
- Node.js LTS installation;
- current Bun and Deno installation;
- managed pnpm installation through Node.js;
- a multi-tool `jolter.json`;
- CI plain logs and JSON output;
- offline reuse after a successful online sync.

## Rollback

If artifacts are invalid or the installer selects a broken release:

1. stop the public installer from selecting the affected version;
2. mark the GitHub release as a prerelease or remove it from normal discovery;
3. preserve logs and artifacts for investigation;
4. publish a corrected patch release instead of replacing immutable artifacts;
5. document user impact and recovery in the changelog and release notes.

Never silently replace an archive while retaining its filename or tag. That
invalidates previously published checksums and breaks reproducibility.

## Post-Release

After publication:

1. install through both public script endpoints;
2. verify checksums and `jolter --version`;
3. monitor provider-smoke and user reports;
4. open the next changelog section;
5. update roadmap status where release evidence closes a milestone.
