# Compatibility Policy

Jolter follows Semantic Versioning.

## Project Configuration

`jolter.json` uses:

```json
{
  "schemaVersion": 1
}
```

Files without `schemaVersion` are interpreted as schema version 1 for backward
compatibility. Unknown schema versions fail closed. Future schema changes must
document migration behavior before they are accepted.

## Storage

The version-1 storage layout is:

```text
~/.jolter/
|-- runtimes/
|-- tools/
|-- shims/
|-- cache/
`-- config/
```

Patch and minor releases in the `0.2.x` line preserve installed runtime, tool,
manifest, active-version, and cache compatibility. A future incompatible
storage change requires an explicit migration command or an automatic atomic
migration.

## Platform Support

Release CI targets Windows x64, Linux x64, macOS x64, macOS ARM64, and WSL.
Runtime providers may impose narrower CPU requirements, such as Bun's SSE4.2
requirement for x64 builds.
