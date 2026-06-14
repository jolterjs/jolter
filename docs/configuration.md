# Configuration

Jolter projects use `jolter.json`. Schema version 1 supports one runtime and
multiple Node.js-based tools.

## Complete Schema

```json
{
  "schemaVersion": 1,
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "npm": "11.x",
    "pnpm": "10.x",
    "yarn": "4.x"
  }
}
```

All fields are optional, but commands such as `sync`, `repair`, and
`setup-ci` require a resolvable runtime from `jolter.json`, `.node-version`, or
`.nvmrc`.

## Top-Level Fields

| Field | Type | Meaning |
| --- | --- | --- |
| `schemaVersion` | integer | Configuration schema. The current and only supported value is `1`. |
| `runtime` | object | Zero or one of `node`, `bun`, or `deno`. |
| `tools` | object | Zero or more of `npm`, `pnpm`, and `yarn`. |

Unknown fields fail validation. Unknown schema versions fail closed rather than
being guessed.

Files without `schemaVersion` are interpreted as schema version 1 for backward
compatibility.

## Runtime Rules

Only one runtime can be configured:

```json
{
  "runtime": {
    "bun": "1.x"
  }
}
```

This is invalid because it declares two runtimes:

```json
{
  "runtime": {
    "node": "24",
    "deno": "2"
  }
}
```

Pinning a different runtime replaces the existing runtime entry:

```bash
jolter pin deno@2
```

## Tool Rules

Multiple tools can be declared:

```json
{
  "runtime": {
    "node": "24"
  },
  "tools": {
    "pnpm": "10",
    "yarn": "4"
  }
}
```

Tools managed by Jolter execute through Node.js. A project with any `tools`
entry must therefore resolve Node.js as its runtime. A Bun or Deno project can
still use that runtime's bundled capabilities, but cannot synchronize Jolter's
managed npm, pnpm, or Yarn entries.

## Selectors

Supported numeric forms are:

| Selector | Meaning |
| --- | --- |
| `24` | Highest stable `24.*.*` release. |
| `24.x` | Same major-line intent as `24`. |
| `24.5` | Highest stable `24.5.*` release. |
| `24.5.x` | Same minor-line intent as `24.5`. |
| `24.5.0` | Exact semantic version. |
| `x` or `*` | Any stable version. |
| `latest` | Highest stable release from the provider. |
| `lts` | Highest current LTS release; Node.js only. |

Leading `v` is accepted in direct requests and normalized. Whitespace,
embedded `@`, ranges such as `>=20`, unions, prerelease syntax, and more than
three numeric components are not supported in schema version 1.

`lts` is rejected for Bun and Deno. Managed tools support numeric selectors,
wildcards, and `latest`, but not `lts`.

## Corepack-Style Tool Hashes

An exact tool version can include an additional archive hash:

```json
{
  "runtime": {
    "node": "24"
  },
  "tools": {
    "pnpm": "10.12.1+sha224.<56-hex-characters>"
  }
}
```

Supported algorithms are `sha1`, `sha224`, `sha256`, `sha384`, and `sha512`.
A hash is accepted only with an exact stable semantic version. Jolter verifies
it in addition to the npm registry's required SHA-512 integrity value.

## Creating and Updating Configuration

Use `pin` for normal edits:

```bash
jolter pin node@24
jolter pin pnpm@10
jolter pin yarn@4
```

`pin` writes the configuration atomically in the current directory. Runtime
pinning preserves tools; tool pinning preserves the runtime and other tools.
Selectors are preserved rather than replaced with the currently resolved exact
version.

Direct editing is supported. Run `jolter doctor` after changing the file.

## Legacy `packageManager` Key

Earlier schema-version-1 files may contain:

```json
{
  "packageManager": {
    "pnpm": "10"
  }
}
```

Jolter continues to read this key as an input alias. The next `pin` operation
writes the canonical `tools` key. New files, documentation, and
machine-readable output use `tools`.

This compatibility key is different from the standard string-valued
`package.json#packageManager` field.

## Recommended Version Policy

For applications, pin a major or minor line to receive compatible updates:

```json
{
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "pnpm": "10.x"
  }
}
```

For controlled build images or release jobs, use exact versions and update
them intentionally. Use `latest` for experimentation, not for workflows that
require the same resolution over time.

Configuration selectors express allowed versions. They are not a lockfile:
the highest matching release can change when provider metadata changes.

## Validation Checklist

Before committing a configuration:

```bash
jolter sync
jolter doctor
```

Commit `jolter.json`, but do not commit `JOLTER_HOME` or installed runtime
directories.
