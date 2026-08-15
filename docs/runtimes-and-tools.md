# Runtimes and Tools

Jolter separates executable JavaScript runtimes from development tools that run
through Node.js.

## Supported Runtimes

| Runtime | Provider                      | Selectors                          | Notes                                                              |
| ------- | ----------------------------- | ---------------------------------- | ------------------------------------------------------------------ |
| Node.js | Official Node.js distribution | Numeric, wildcard, `latest`, `lts` | Provides the execution environment for managed tools.              |
| Bun     | Official Bun GitHub releases  | Numeric, wildcard, `latest`        | x64 builds require the CPU capabilities supported by the provider. |
| Deno    | Official Deno GitHub releases | Numeric, wildcard, `latest`        | Installed as an independent runtime.                               |

Jolter selects artifacts for the current operating system and architecture.
Unsupported platform combinations fail before publication.

## Supported Managed Tools

| Tool | Registry package    | Commands     |
| ---- | ------------------- | ------------ |
| npm  | `npm`               | `npm`, `npx` |
| pnpm | `pnpm`              | `pnpm`       |
| Yarn | `@yarnpkg/cli-dist` | `yarn`       |

Tool tarballs come from the official npm registry and execute through the
selected Node.js runtime. Jolter validates the package's `engines.node` range
before installation reuse or activation.

## Global Activation

`jolter use` creates a machine-wide default inside `JOLTER_HOME`:

```bash
jolter use node@24
jolter use pnpm@10
```

The resolved exact versions become active fallbacks. They are used outside a
project or when the current project has no requirement for that command.

Global activation does not edit system Node.js installations, registry state,
or project files.

## Installation Without Activation

`jolter install` (or `jolter i`) downloads and installs a runtime or tool locally into `JOLTER_HOME` storage without setting it as the globally active version:

```bash
jolter install node@24.1.0
jolter i pnpm@10.2.0
```

This is useful when pre-fetching toolchain dependencies for local testing or automation without altering your machine-wide active selection.

## Project Selection

Project requirements override global activation:

```json
{
  "runtime": {
    "node": "22"
  },
  "tools": {
    "pnpm": "9"
  }
}
```

Inside that project, the shims select a matching installed Node.js 22 and pnpm
9 even if Node.js 24 and pnpm 10 are globally active.

Selection does not install during command dispatch. Run `jolter sync` first.
If a required version is missing, the shim fails with a remediation message
instead of silently downloading while another command is starting.

## Resolution and Reuse

For a numeric selector, Jolter reuses the highest complete local match where
the operation permits reuse. `latest` and Node.js `lts` require release
metadata to establish the current provider result.

`jolter update <name>` deliberately checks the active major line for a newer
release. An explicit exact version can be reused locally.

## npm and Node.js

Official Node.js archives include bundled `npm` and `npx` commands. Jolter can
route those bundled commands through the selected Node.js runtime when no
managed npm override applies.

Pinning or globally activating an npm tool version creates an explicit managed
override. This allows a project to use a different npm release from the one
bundled with its selected Node.js version.

## Tool Compatibility

Before using a managed tool, Jolter:

1. identifies the selected exact Node.js version;
2. reads the tool's declared Node.js engine range from package metadata;
3. rejects an incompatible pairing;
4. dispatches the tool's JavaScript entry point through that Node.js binary.

This check also runs when reusing an existing tool installation, preventing a
previously installed version from being silently paired with an incompatible
runtime.

## Integrity and Manifests

Runtime archives are verified with provider SHA-256 data. Managed tools are
verified with npm registry SHA-512 SRI metadata. Exact Corepack-style hashes
are an additional check, never a replacement for registry integrity.

Published installations include internal Jolter manifests used by inventory,
repair, and diagnostics. Do not edit these manifests manually.

## Choosing Selectors

- Use `node@lts` for a personal default that follows the current LTS.
- Use `node@24` or `node@24.x` for a project that should follow a major line.
- Use exact versions for release images requiring a fixed resolution.
- Use tool major lines such as `pnpm@10` for normal project compatibility.
- Use `latest` only where provider movement is acceptable.

See [configuration](configuration.md) for the complete grammar.
