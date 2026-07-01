# Compatibility Policy

Jolter follows Semantic Versioning while acknowledging that releases before
`1.0.0` may still evolve. Explicit compatibility promises below apply to the
`0.2.x` line.

## User-Facing Contract

Patch releases in `0.2.x` should preserve:

- documented command names and required argument forms;
- schema-version-1 project configuration;
- installed runtime and tool discovery;
- active exact-version state;
- documented machine-readable field names;
- supported platform artifacts;
- legacy inputs explicitly listed in this policy.

Human wording, progress animation, colors, spacing, and provider metadata cache
contents are not stable parsing interfaces.

Automation should use JSON, process exit status, and documented CI outputs.

## Project Configuration

Canonical schema:

```json
{
  "$schema": "https://schemas.jolter.dev/project/v1/schema.json",
  "schemaVersion": 1,
  "runtime": {
    "node": "24"
  },
  "tools": {
    "pnpm": "10"
  }
}
```

Files without `$schema` remain accepted. Files without `schemaVersion` are
interpreted as the current schema. Unknown versions fail closed. Unknown fields
also fail validation.

Schema version 1 guarantees:

- zero or one runtime from Node.js, Bun, and Deno;
- zero or more supported managed tools;
- selector validation before installation;
- atomic writes by `jolter pin`.

The earlier map-valued `packageManager` key remains an accepted input alias.
Whenever Jolter writes that configuration again, it emits canonical `tools`.

## Compatibility Files

Jolter supports:

- `.node-version`;
- `.nvmrc`;
- standard string-valued `package.json#packageManager`.

These are interoperability sources, not Jolter-owned schemas. Jolter supports
the subset documented in [configuration](configuration.md) and
[project resolution](project-resolution.md); it does not promise to accept
every selector syntax another manager may support.

## Storage

Version-1 layout:

```text
~/.jolter/
|-- runtimes/
|-- tools/
|-- shims/
|-- cache/
`-- config/
```

The `0.2.x` line preserves discovery of:

- exact-version runtime directories;
- exact-version tool directories;
- `.jolter-install.json`;
- `.jolter-tool.json`;
- `config/active.json`;
- legacy tool manifest fields read by diagnostics.

New tool manifests use `tool` and `toolHash`. Diagnostics continue to accept
the previous `packageManager` field names.

Cache entries are disposable and can change without migration because they are
rebuildable. Lock filenames and temporary staging names are internal.

## Machine Output

Documented JSON top-level fields are:

| Command           | Fields                                           |
| ----------------- | ------------------------------------------------ |
| `list --json`     | `runtimes`, `tools`                              |
| `doctor --json`   | `healthy`, `checks`                              |
| `setup-ci --json` | `provider`, `runtime`, `tools`, `shims`, `cache` |

New optional fields may be added in a compatible release. Consumers should
ignore unknown fields and should not depend on object-key order.

Removing or changing the type or meaning of a documented field requires a
versioned compatibility decision.

## CI Outputs

GitHub Actions outputs currently include:

- `runtime`;
- `tools`;
- `cache`.

`tools` is a comma-separated list. A project with no managed tools may omit
that output. Scripts should handle its absence.

## Platform Support

Release CI targets:

- Windows x64;
- Linux x64;
- macOS Intel x64;
- macOS Apple Silicon;
- WSL through the Linux build.

Provider support may be narrower than Jolter's binary support. CPU or artifact
availability imposed by Node.js, Bun, Deno, or npm packages is not a Jolter
compatibility guarantee.

## Provider Behavior

Jolter preserves selector meaning, integrity enforcement, and stable-release
selection. It does not control:

- provider availability;
- release publication or withdrawal;
- upstream engine requirements;
- registry rate limits;
- the exact version returned by moving selectors over time.

Use exact versions when provider movement is unacceptable.

## Deprecation and Migration

Before removing a compatibility input or changing persistent storage, a future
release must provide:

1. documentation of the old and new behavior;
2. a migration path;
3. atomic or safely restartable migration;
4. diagnostics for unsupported state;
5. release notes with recovery instructions.

An incompatible storage change requires an explicit migration command or an
automatic atomic migration. Silent deletion or reinterpretation of installed
state is not acceptable.

## Reporting Compatibility Bugs

Include the Jolter version, platform, relevant configuration, command, expected
behavior, actual output, and whether the state was created by an earlier
release. Remove credentials and private paths before sharing logs publicly.
