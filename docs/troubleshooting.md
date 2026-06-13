# Troubleshooting

Start with:

```bash
jolter doctor
```

Each warning or failure includes an action when Jolter can identify one.

## Command Resolves Outside Jolter

Inspect `PATH precedence` in `jolter doctor`. Move the Jolter shims directory
before system Node.js, nvm, fnm, Volta, or Corepack entries.

## Required Version Is Missing

```bash
jolter sync
```

For an incomplete existing directory:

```bash
jolter repair
```

## Offline Cache Miss

Disable `JOLTER_OFFLINE`, run `jolter sync` once with network access, then
reenable offline mode. `jolter cache clean` deliberately removes offline
readiness.

## Proxy Or Certificate Failure

Check `HTTPS_PROXY`, `HTTP_PROXY`, `NO_PROXY`, `SSL_CERT_FILE`,
`NODE_EXTRA_CA_CERTS`, and `REQUESTS_CA_BUNDLE`. Proxy URLs must use `http://`
or `https://`, and certificate variables must point to existing files.

## Storage Permission Failure

Fix ownership and write permissions for `JOLTER_HOME`, or choose a writable
location:

```bash
export JOLTER_HOME="$HOME/.jolter"
```

PowerShell:

```powershell
$env:JOLTER_HOME = "$HOME\.jolter"
```
