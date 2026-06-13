# Shell Setup

Jolter switches toolchains through shims. The shims directory must be on
`PATH` before commands such as `node`, `pnpm`, or `deno` can be resolved by
Jolter.

Run:

```bash
jolter setup
```

The command:

1. creates or refreshes every Jolter shim;
2. checks whether the shims directory is already on the current process
   `PATH`;
3. prints an exact command for the current shell session;
4. prints an exact command for persistent user configuration.

Jolter does not silently edit shell profiles or the Windows user environment.
The printed persistent command makes that change only when the user runs it.

## Shell Selection

Automatic selection defaults to PowerShell on Windows. On macOS and Linux,
Jolter inspects `SHELL` and recognizes Bash, Zsh, and Fish.

Selection can be explicit:

```bash
jolter setup --shell powershell
jolter setup --shell cmd
jolter setup --shell bash
jolter setup --shell zsh
jolter setup --shell fish
```

## Verification

After applying the persistent command, restart the shell and run:

```bash
jolter doctor
```

The `PATH` check should report `ok`. Installed versions and incomplete
installation directories can be inspected with:

```bash
jolter list
```

## Custom Storage

When `JOLTER_HOME` is set, `setup` installs shims under:

```text
$JOLTER_HOME/shims
```

The generated instructions always use the resolved absolute shims path.

## Completions

Generate completions without modifying shell profiles:

```bash
jolter completions bash
jolter completions zsh
jolter completions fish
jolter completions elvish
jolter completions powershell
```

Redirect the output into the completion location used by the selected shell.
Command Prompt does not provide a comparable native programmable-completion
interface.
