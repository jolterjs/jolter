## Description

Provide a summary of the changes made and the motivation behind them. If this pull request resolves or fixes an existing issue, link it here (e.g. `Fixes #123`).

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Refactoring / Performance improvement
- [ ] Documentation update
- [ ] CI / Workflow changes

## Quality Gates Checklist

Before submitting, please ensure the following checks pass locally:

- [ ] `cargo fmt --all -- --check` passes cleanly
- [ ] `cargo clippy --workspace --all-targets --locked -- -D warnings` has no warnings
- [ ] `cargo test --workspace --all-targets --locked` passes all tests
- [ ] Code changes include corresponding unit or integration tests (where applicable)
- [ ] Relevant documentation in `docs/` or `jolter.dev/docs` has been updated

## Test Plan

Describe how you tested your changes locally (e.g. commands run, isolated `$env:JOLTER_HOME` or `$JOLTER_HOME` test execution, OS platforms tested).
