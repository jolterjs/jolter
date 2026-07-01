# Diagnostics and Inventory

Jolter provides two complementary inspection commands:

- `jolter list` reports what exists in managed storage;
- `jolter doctor` evaluates whether the current environment and project are
  usable.

Both commands are non-destructive.

## Inventory

```bash
jolter list
```

The output is separated into runtimes and tools. Columns are dynamically sized
from the installed labels and paths, so status and path values remain aligned
without terminal tab stops.

Example:

```text
Runtimes:
* node@24.5.0  [ready]       C:\Users\me\.jolter\runtimes\node\24.5.0
  node@22.9.0  [incomplete]  C:\Users\me\.jolter\runtimes\node\22.9.0

Tools:
* pnpm@10.12.1 [ready]       C:\Users\me\.jolter\tools\pnpm\10.12.1
```

Interpretation:

- `*` means the exact version is globally active;
- `ready` means the expected executable or entry point exists;
- `incomplete` means a semantic-version directory exists but its required
  payload is missing.

Inventory does not hide incomplete directories. This is intentional: partial
or externally modified installations remain visible for repair and cleanup.

## Inventory JSON

```bash
jolter list --json
```

Top-level fields are `runtimes` and `tools`. Each record contains:

| Field     | Type    | Meaning                                          |
| --------- | ------- | ------------------------------------------------ |
| `kind`    | string  | `node`, `bun`, `deno`, `npm`, `pnpm`, or `yarn`. |
| `version` | string  | Exact semantic version.                          |
| `path`    | string  | Managed installation path.                       |
| `ready`   | boolean | Expected executable or entry point exists.       |
| `active`  | boolean | Exact version is the global fallback.            |

Use JSON for automation. Human table wording and style are not a parsing
contract.

## Health Checks

```bash
jolter doctor
```

Doctor checks include:

- resolved storage location and writability;
- operating system and architecture support;
- Bun CPU compatibility where relevant;
- project configuration and compatibility-file resolution;
- required runtime and tool availability;
- bounded `--version` probes for installed selections;
- installation manifests and executable permissions;
- managed tool entry points and Node.js engine compatibility;
- shim presence;
- shim directory presence and precedence on `PATH`;
- shadowing by common system and version-manager paths;
- incomplete or unrecognized cache entries;
- offline cache readiness;
- proxy and certificate environment configuration.

Each result is:

- `pass`: the checked condition is ready;
- `warning`: a risk or non-blocking concern was found;
- `fail`: the current environment or project cannot be used as expected.

Warnings do not fail the process. Any failed check produces a nonzero exit
status.

## Remediation

Human findings can include:

```text
action: run `jolter sync`
```

Apply remediation in order:

1. storage and permission failures;
2. invalid project configuration;
3. missing or incomplete runtime requirements;
4. missing or incompatible tools;
5. shims and `PATH` precedence;
6. cache, proxy, and certificate warnings.

Rerun `jolter doctor` after each environmental change.

## Doctor JSON

```bash
jolter doctor --json
```

Example:

```json
{
  "checks": [
    {
      "message": "using /home/user/.jolter",
      "name": "storage",
      "status": "pass"
    },
    {
      "message": "Jolter shims are not first on PATH",
      "name": "PATH precedence",
      "remediation": "move /home/user/.jolter/shims before other tool managers",
      "status": "warning"
    }
  ],
  "healthy": true
}
```

JSON uses the same checks and exit-status rules as human output.

## Probe Safety

Doctor can execute an already-installed runtime or tool with `--version`.
Probes:

- never execute a downloaded archive during installation;
- have a five-second timeout;
- capture at most 16 KiB from standard output and standard error;
- set `JOLTER_DOCTOR=1` for the child process.

Review this behavior before running diagnostics on a storage directory obtained
from an untrusted source.

## Common Diagnostic Workflows

New machine:

```bash
jolter setup
jolter doctor
```

Project onboarding:

```bash
jolter sync
jolter doctor
```

Suspected corruption:

```bash
jolter list
jolter repair
jolter doctor
```

Automation evidence:

```bash
jolter doctor --json --no-color > jolter-doctor.json
```

Do not redirect standard error into the JSON file.
