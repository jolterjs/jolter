# Installation

## Release Archives

Tagged releases publish archives for Windows x64, Linux x64, macOS x64, and
macOS ARM64. Each archive has a sibling `.sha256` file. Verify the checksum,
extract the archive, then place `jolter` or `jolter.exe` on `PATH`.

## Build From Source

Rust `1.85` or newer is required:

```bash
cargo build --release --locked
```

The executable is written under `target/release`.

## First Run

```bash
jolter setup
jolter use node@lts
jolter doctor
```

Apply the shell-specific `PATH` command printed by `jolter setup`, restart the
shell, and rerun `jolter doctor`.

## Upgrade

Replace the Jolter executable with the newer release and run:

```bash
jolter setup
jolter doctor
```

Storage and project configuration schema version 1 are compatible across the
`0.2.x` series.
