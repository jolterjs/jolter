# Project Resolution

Jolter resolves requirements from the current working directory upward. This
allows commands to work from nested package directories without a shell hook.

## Runtime Precedence

Runtime sources are considered in this order:

1. the nearest ancestor `jolter.json` containing a runtime;
2. the nearest ancestor `.node-version`;
3. the nearest ancestor `.nvmrc`;
4. the nearest ancestor `package.json#devEngines.runtime`;
5. the globally active runtime for direct shim execution.

The source type has priority. For example, any discovered `.node-version` wins
over `.nvmrc`, even if the `.nvmrc` is closer to the current directory.

`sync`, `repair`, and `setup-ci` require a project runtime from the first four
sources. They do not treat a global activation as a project declaration.

## Tool Precedence

Tool sources are considered in this order:

1. the `tools` map in the discovered `jolter.json`, when it is non-empty;
2. `package.json#devEngines.packageManager`;
3. standard string-valued `package.json#packageManager` field;
4. the globally active tool for direct shim execution;
5. bundled npm or npx from the selected Node.js runtime when applicable.

When `jolter.json` declares any tools, `package.json` compatibility fields are not
merged with them. Put every intended managed tool in the `tools` map.

## File Discovery

Each supported filename is searched from the current directory toward the
filesystem root. The first matching file of that name is selected.

Example:

```text
repository/
|-- jolter.json
|-- apps/
|   `-- web/
|       `-- src/
`-- packages/
```

Running from `repository/apps/web/src` discovers
`repository/jolter.json`.

The directory containing `jolter.json` becomes the project root in resolution
results. Without a Jolter configuration, the command's starting directory is
used as the operational root while compatibility files are still discovered
upward.

## `package.json#devEngines` and `packageManager`

Jolter provides first-class support for `devEngines` in `package.json`:

```json
{
  "devEngines": {
    "runtime": {
      "name": "node",
      "version": "^20.0.0",
      "onFail": "error"
    },
    "packageManager": {
      "name": "pnpm",
      "version": "^11.17.0",
      "onFail": "download"
    }
  }
}
```

Legacy string-valued `packageManager` compatibility fields are also accepted:

```json
{
  "packageManager": "pnpm@10.12.1"
}
```

Exact Corepack descriptors are accepted:

```json
{
  "packageManager": "pnpm@10.12.1+sha224.<hash>"
}
```

Malformed fields fail resolution rather than being ignored. Only one tool can
be represented by this compatibility source; use `jolter.json` for multiple
tools.

Command shims are generated demand-driven when runtimes, tools, or plugins are installed.
When a shimmed command is invoked, Jolter:

1. identifies the command name;
2. resolves the current project's requirements;
3. chooses the highest complete local installation matching the project;
4. falls back to the exact globally active version when no project
   requirement applies;
5. launches the real executable or tool entry point.

No background daemon, shell function, or directory-change hook is required.

## Installation Is Explicit

Shims resolve and dispatch; they do not download missing versions. Prepare a
project with:

```bash
jolter sync
```

This makes command startup deterministic and avoids an unexpected network
operation in the middle of a build.

## Monorepo Guidance

Place `jolter.json` at the highest directory that should share one toolchain.
Add a nested `jolter.json` only when that subtree intentionally requires a
different runtime or tool set.

Remember that source categories have fixed precedence. A root
`jolter.json` runtime overrides nested `.node-version` files below it. Use
nested Jolter configuration when an explicit boundary is required.

## Diagnosing Resolution

Run from the directory whose behavior matters:

```bash
jolter doctor
```

The project resolution check reports invalid files, unsupported selectors, and
missing requirements. Then use:

```bash
jolter list
jolter sync
```

to compare local inventory and prepare the selected project.
