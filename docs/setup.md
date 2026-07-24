# Shell Setup and Automatic Switching

Jolter switches versions through self-shims. A shim is a Jolter executable
installed under a supported command name. It resolves the current project and
launches the selected real runtime or tool.

## Install Shims

```bash
jolter setup
```

The command:

1. creates the Jolter storage layout if needed;
2. refreshes command shims matching installed runtimes and tools;
3. checks whether the shims directory is on the current `PATH`;
4. prints a command for the current session;
5. prints a persistent user-level command for the selected shell.

Jolter does not silently rewrite profile files or the Windows user
environment.

## Shim Location

Default:

```text
Windows: %USERPROFILE%\.jolter\shims
Unix:   $HOME/.jolter/shims
```

With a custom home:

```text
$JOLTER_HOME/shims
```

Command shims are created demand-driven when a runtime, tool, or plugin tool is installed (globally or for a project):

```text
node, npm, npx  (created when a Node.js runtime or npm tool is installed)
pnpm            (created when a pnpm tool is installed)
yarn            (created when a yarn tool is installed)
bun             (created when a Bun runtime is installed)
deno            (created when a Deno runtime is installed)
<plugin-tool>   (created when a plugin tool is installed)
```

Platform-specific executable suffixes are handled automatically.

## Shell Selection

```bash
jolter setup --shell powershell
jolter setup --shell cmd
jolter setup --shell bash
jolter setup --shell zsh
jolter setup --shell fish
```

`--shell auto` is the default. Windows defaults to PowerShell. On macOS and
Linux, Jolter inspects `SHELL` for Bash, Zsh, or Fish.

Use an explicit value when running from an embedded terminal, CI shell, or a
shell whose parent process does not expose reliable detection.

## PATH Precedence

The shims directory must appear before competing executables from:

- system Node.js installations;
- nvm or nvm-windows;
- fnm;
- Volta;
- Corepack shims;
- package-manager-specific global binary directories.

Check effective command resolution:

PowerShell:

```powershell
Get-Command node
Get-Command pnpm
```

Command Prompt:

```batch
where node
where pnpm
```

Bash or Zsh:

```bash
type -a node
type -a pnpm
```

Fish:

```fish
type -a node
type -a pnpm
```

The first result should be inside the Jolter shims directory.

## Verify Setup

Restart the shell after applying persistent configuration:

```bash
jolter doctor
jolter list
```

The doctor checks both presence and precedence. A shim can exist and still be
ineffective when another manager appears earlier on `PATH`.

## How Switching Works

Every invocation resolves the current directory:

```bash
cd project-a
node --version

cd ../project-b
node --version
```

No directory-change hook or resident daemon is needed. Project requirements
override globally active versions; global versions provide a fallback outside
configured projects.

Shims do not install missing versions. Prepare each project with:

```bash
jolter sync
```

See [project resolution](project-resolution.md) for precedence details.

## Custom Storage

Set `JOLTER_HOME` before running Jolter:

Bash or Zsh:

```bash
export JOLTER_HOME="$HOME/.local/share/jolter"
jolter setup
```

PowerShell:

```powershell
$env:JOLTER_HOME = "$HOME\AppData\Local\Jolter"
jolter setup
```

The setting must be consistent across setup, interactive shells, IDEs, and CI.
Changing it points Jolter at a different set of shims, installations, cache,
and active versions.

## IDE Terminals and GUI Applications

An IDE launched before a persistent `PATH` update can retain the old
environment. Restart the entire application, not only its integrated terminal.

GUI applications on macOS and Linux may not read interactive shell profiles.
Use the environment configuration supported by the application or operating
system, then confirm command resolution inside the application terminal.

## Shell Completions

Generate a completion script:

```bash
jolter completions bash
jolter completions zsh
jolter completions fish
jolter completions elvish
jolter completions powershell
```

The script is written to standard output. Redirect it to the completion
directory recommended by the shell, or source it from a profile managed by the
user. Jolter does not modify profiles automatically.

Command Prompt has no equivalent native programmable-completion integration.

## Refreshing Setup

Run setup again after:

- upgrading or moving the Jolter executable;
- changing `JOLTER_HOME`;
- restoring storage from backup;
- adding support for new shimmed commands in a future release;
- diagnosing missing or stale shim files.

```bash
jolter setup
jolter doctor
```
