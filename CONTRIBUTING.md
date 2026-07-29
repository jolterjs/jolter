# Contributing to Jolter

Thank you for your interest in contributing to Jolter! We welcome contributions of all kinds, including bug reports, documentation improvements, feature requests, and code contributions.

Please take a moment to review this document before submitting your contribution.

---

## Code of Conduct

All contributors are expected to adhere to our [Code of Conduct](CODE_OF_CONDUCT.md). Please read it to understand the community standards and enforcement guidelines.

---

## Getting Started

### Prerequisites

- **Rust**: Version `1.85` or newer (edition 2024 support required)
- **Cargo**
- **Git**
- **C/C++ Build Tools**: Platform-specific toolchain for compiling native dependencies

### Setting Up the Repository

1. **Clone the repository**:

   ```bash
   git clone https://github.com/jolterjs/jolter.git
   cd jolter
   ```

2. **Build the workspace**:

   ```bash
   cargo build --workspace --all-targets --locked
   ```

3. **Located development binaries**:
   - **Windows**: `target/debug/jolter.exe`
   - **Linux / macOS**: `target/debug/jolter`

---

## Quality Gates

Before opening a pull request, run all quality checks to ensure continuous integration passes:

### 1. Code Formatting

```bash
cargo fmt --all -- --check
```

### 2. Clippy Lints

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

### 3. Tests

```bash
cargo test --workspace --all-targets --locked
```

### 4. Test Coverage (Optional)

```bash
cargo llvm-cov --workspace --all-targets --locked --fail-under-lines 80
```

### 5. Dependency Audit

```bash
cargo audit
```

---

## Isolated Manual Testing

To avoid modifying your personal `$HOME/.jolter` directory during development, run Jolter using an isolated directory:

**Bash / Zsh (Linux & macOS)**:

```bash
export JOLTER_HOME="$(mktemp -d)"
cargo run -p jolter-cli --bin jolter -- use node@24
```

**PowerShell (Windows)**:

```powershell
$env:JOLTER_HOME = Join-Path $env:TEMP "jolter-dev"
cargo run -p jolter-cli --bin jolter -- use node@24
```

---

## Pull Request Workflow

1. **Fork & Branch**: Create a feature or fix branch from `main`.
   ```bash
   git checkout -b fix/my-fix-name
   ```
2. **Commit Messages**: Write clear, descriptive commit messages.
3. **Run Quality Gates**: Ensure formatting, clippy, and tests pass cleanly.
4. **Submit PR**: Open a pull request against the `main` branch. Fill out the [Pull Request Template](.github/PULL_REQUEST_TEMPLATE.md).
5. **Review**: A maintainer will review your pull request and provide feedback.

---

## Security Concerns

If you discover a security vulnerability, please **do not** open a public issue. Refer to our [Security Policy](SECURITY.md) to report security issues privately.
