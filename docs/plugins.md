# Plugins

Jolter v0.3 can install registry-hosted plugins globally and use them from
project configuration.

Plugins use scoped registry names such as `@eslint/eslint`. Verified ecosystem
plugins may also have an official unscoped alias such as `eslint`.

## Install

```bash
jolter plugin install eslint
jolter plugin install @eslint/eslint@1.x
```

Jolter resolves aliases through the registry, selects a matching plugin
version, downloads the release manifest and WASM entrypoint, verifies the
declared SHA-256 and size, and stores the result under `JOLTER_HOME/plugins`.

## Project Configuration

Schema version 2 adds a `plugins` section:

```json
{
  "$schema": "https://schemas.jolter.dev/project/v2/schema.json",
  "schemaVersion": 2,
  "runtime": {
    "node": "24.x"
  },
  "tools": {
    "eslint": "8.x"
  },
  "plugins": {
    "eslint": "1.x"
  }
}
```

`tools` selects the tool version used by the project. `plugins` selects the
Jolter plugin version that supports that tool.

Built-in tools (`npm`, `pnpm`, and `yarn`) do not require plugin declarations.
Unknown tools are accepted only in schema version 2 when the project declares
plugins.

## Sync

`jolter sync` verifies that declared plugins are already installed. To allow
Jolter to install missing declared plugins, pass:

```bash
jolter sync --yes
jolter setup-ci --yes
```

Without `--yes`, non-interactive runs fail with remediation text instead of
silently installing new plugin code.

## Development Toolkit

Use `jolter-development-toolkit` to create plugin release artifacts:

```bash
npm install -D jolter-development-toolkit
npx jdt init
npx jdt build
npx jdt run list-tools
npx jdt run resolve-tool example latest
npx jdt pack --version 1.0.0
```

`jdt pack` writes `dist/plugin.wasm`, `dist/plugin.release.json`, and
`dist/checksums.txt`. `jdt run` executes the local plugin API and validates its
JSON output before the plugin is published.
