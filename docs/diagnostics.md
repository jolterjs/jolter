# Diagnostics and Inventory

## Installed Toolchains

Run:

```bash
jolter list
```

The command lists both runtime and managed package manager directories.

- `*` marks a globally active exact runtime or package manager.
- `ready` means the expected executable or package manager entry point exists.
- `incomplete` means a version directory exists but its expected entry point
  is missing.

Incomplete project requirements can be restored with:

```bash
jolter repair
```

## Health Checks

Run:

```bash
jolter doctor
```

Checks currently cover:

- storage location;
- storage writability;
- operating-system, architecture, and Bun CPU support;
- project requirement resolution;
- runtime availability;
- bounded runtime `--version` probing;
- runtime installation manifests and executable permissions;
- managed package manager availability;
- bounded package manager `--version` probing;
- package manager manifests and Node.js engine compatibility;
- installed shims;
- shims directory presence and precedence on `PATH`;
- unrecognized cache entries and offline readiness;
- proxy and certificate environment configuration.

Warnings do not make the command fail. A failed check produces a nonzero exit
status.

Human-readable findings include an `action:` line when remediation is
available. JSON findings include the same text in an optional `remediation`
field.

## JSON Output

Automation can use:

```bash
jolter doctor --json
```

Example shape:

```json
{
  "checks": [
    {
      "message": "using /home/user/.jolter",
      "name": "storage",
      "status": "pass"
    }
  ],
  "healthy": true
}
```

Check status values are:

- `pass`
- `warning`
- `fail`

The JSON command uses the same exit status rules and checks as the
human-readable command.

Version probing executes only already-installed local tools, never a newly
downloaded archive during installation. Probes have a five-second timeout and
bounded captured output.
