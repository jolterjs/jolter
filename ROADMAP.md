# The Road to Jolter 2027

This roadmap represents our current vision for Jolter leading up to the first stable yearly release (`2027.0.0`).

Our goal is not to ship as many features as possible, but to build the most polished, reliable and extensible universal developer tool manager.

> **Note**
> This roadmap is subject to change as development progresses.

---

# v0.4.0 — Foundation

The primary goal of v0.4.0 is improving the internal architecture of Jolter.

Rather than introducing many user-facing features, this release focuses on making the project significantly easier to maintain and extend.

## Planned

- major internal architecture rewrite
- split large source files into maintainable modules
- reduce technical debt
- improve crate boundaries
- improve internal APIs
- improve testability
- continue increasing code quality

---

# v0.5.0 — Ecosystem

v0.5.0 focuses on growing the Jolter ecosystem.

## Core Feature

- Global configuration system

Examples:

```bash
jolter config set ...
jolter config get ...
jolter config list
```

Similar to:

- git config
- npm config
- cargo config

## Official Plugin Initiative

We will begin publishing and maintaining official plugins under:

```
github.com/jolterjs
```

Goals:

- official plugins for popular developer tools
- automated plugin updates
- automatic releases for new upstream versions
- optional transfer of plugins to original maintainers
- first-class plugin quality

---

# Between v0.5.0 and v0.6.0

## Benchmark Project

A public benchmarking suite comparing:

- Jolter
- mise
- Volta
- fnm
- nvm

The benchmark suite will remain open-source and transparent.

---

# v0.6.0 — Performance

The goal of v0.6.0 is making Jolter significantly faster.

## Planned

- archive verification improvements
- SHA verification improvements
- tarball improvements
- extraction improvements
- download pipeline optimizations
- additional internal rewrites
- lower memory usage

---

# v0.7.0 — Registries

## Custom Registries

Users will be able to create their own registries.

Supported use cases:

- enterprise registries
- private company plugins
- internal developer tooling
- public community registries

---

## Provider Namespaces

Support installing tools from multiple providers.

Examples:

```text
node
jt:node
jolter:node

npm:eslint
npm:typescript

github:owner/repository

mise:node
mise:bun
```

This creates a universal provider system while keeping the Jolter registry the default.

---

# v0.8.0 — Quality

A release dedicated to stability.

## Planned

- performance improvements
- additional rewrites
- code cleanup
- higher test coverage

Current minimum:

```
80%
```

Target:

```
85–90%
```

---

# v0.9.0 — Compatibility

One of the final major releases before stable.

## Planned

### Mise Registry Compatibility

Support the Mise registry to dramatically reduce migration barriers.

Our goal is making switching to Jolter as simple as possible.

---

# v0.9.5 — Final Polish

The final pre-stable milestone.

## Planned

- performance improvements
- architecture cleanup
- release polishing
- additional testing
- GitHub Action

### @jolterjs/setup-jolter

Official GitHub Action for CI/CD.

---

# 2027.0.0 — First Stable Release

The first yearly stable release.

Versioning changes from:

```
0.x.y
```

to

```
2027.x.y
```

This release represents:

- stable APIs
- production readiness
- polished developer experience
- mature architecture
- stable plugin ecosystem
- enterprise-ready registry system

---

# 2027.x — Platform Expansion

Following the first stable release, development will continue throughout the 2027 release cycle.

## Planned

### Task Runner

Integrated task execution.

Example:

```bash
jolter run build
jolter run test
```

---

### Environment Manager

Unified environment management.

Examples:

- project environments
- environment profiles
- runtime-aware execution

---

Additional quality improvements, performance work and ecosystem growth will continue throughout the 2027 release cycle.

---

# Towards 2028

Our goal is for the beginning of the 2028 release cycle to represent a "feature complete" core platform.

Future development will primarily focus on:

- ecosystem growth
- additional providers
- enterprise features
- performance
- developer experience
- new official plugins

rather than introducing fundamental platform capabilities.

---

# Long-Term Vision

Jolter aims to become the universal developer environment manager.

Core pillars:

- Universal tool management
- Official plugin ecosystem
- Registry-first architecture
- Enterprise-ready registries
- Excellent developer experience
- Performance
- Stability
- Transparency

Every feature should support one of these goals.

Our objective is not simply to provide another version manager, but to build a modern, polished platform that developers can confidently rely on every day.
