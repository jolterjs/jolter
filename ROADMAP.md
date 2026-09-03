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

I’d rewrite **v0.5.0** quite substantially. The old “Official Plugin Initiative” no longer fits the direction we discussed; v0.5.0 should instead establish the **instruction-registry architecture, declarative tool support, and configuration format expansion**.

I’d also slightly rename the release from **Ecosystem** to **Registry & Configuration**, because that better describes the architectural purpose.

# v0.5.0 — Registry & Configuration

v0.5.0 is a major architectural evolution of Jolter.

The goal of this release is to remove the distinction between "built-in" and plugin-provided tools and establish a registry-driven architecture where tools and runtimes are defined through declarative instructions rather than hard-coded implementations or installed plugins.

This release also expands Jolter's configuration system and lays the foundation for a much larger ecosystem.

## Instruction Registry

Jolter will introduce a centralized instruction registry describing how tools and runtimes can be resolved, downloaded, verified and installed.

Instead of requiring a plugin to be installed before a tool can be used, Jolter will resolve the tool through its instruction.

For example:

```bash
jolter install eslint
jolter pin node@24
jolter sync
```

will resolve the required instructions automatically.

The registry will describe tools such as:

* Node.js
* Bun
* Deno
* npm
* pnpm
* Yarn
* other developer tools

The goal is for these tools to be treated identically by Jolter's resolver.

There will no longer be a concept of "built-in supported tools" in the core architecture.

## Removal of Hard-Coded Tool Implementations

The existing hand-written implementations for:

* Node.js
* Bun
* Deno
* npm
* pnpm
* Yarn

will be removed from Jolter's core tool handling.

Their installation and resolution behavior will instead be represented through registry instructions.

This creates a generic architecture where adding support for a new tool does not require modifying Jolter's core resolver.

The architectural goal is:

```text
Tool
  ↓
Instruction Registry
  ↓
Resolver
  ↓
Installer
```

rather than:

```text
Tool
  ↓
Hard-coded implementation
```

## Plugin System Rewrite

The existing plugin installation model will be replaced as the primary mechanism for adding tool support.

Users will no longer need to install a plugin simply to make a tool available to Jolter.

Instead:

```text
jolter install <tool>
```

will resolve the appropriate instruction automatically.

Plugins may remain as an advanced extension mechanism for functionality that cannot be represented declaratively by the instruction system.

The distinction becomes:

* **Instructions** extend Jolter's knowledge of tools.
* **Plugins** extend Jolter's capabilities.

This significantly reduces the amount of plugin management required from users and simplifies the core architecture.

---

## Project Configuration

Jolter's project configuration model will be revised to better represent its actual semantics.

### Singular Runtime

Since a Jolter configuration currently supports exactly one project runtime, the configuration will use `runtime` rather than `runtimes`.

Example:

```toml
[runtime]
node = "24"

[tools]
pnpm = "10"
eslint = "9"
```

The runtime will continue to support different runtime implementations through the instruction registry.

For example:

```toml
[runtime]
node = "24"
```

and:

```toml
[runtime]
bun = "1.2"
```

are both resolved through the same generic mechanism.

The configuration itself does not need to know how the runtime is installed.

---

## Additional Configuration Formats

Jolter will expand support beyond its existing project configuration format.

Initial focus:

* TOML
* YAML
* JSON

Additional formats may be introduced as the configuration architecture matures.

All supported formats will resolve into the same internal Jolter project configuration model.

For example:

```toml
[runtime]
node = "24"

[tools]
pnpm = "10"
eslint = "9"
```

and an equivalent JSON configuration should produce the same project requirements.

Configuration format and tool resolution will remain separate concerns.

---

## Generic Tool Model

Jolter's internal representation of tools and runtimes will become generic.

The core resolver will no longer contain special cases for individual tools.

Instead, it will operate on requirements such as:

```text
runtime:
  node → 24

tools:
  pnpm → 10
  eslint → 9
```

and resolve those requirements using registry instructions.

This provides the foundation for supporting a significantly larger number of developer tools without continuously expanding Jolter's core codebase.

---

## Global Configuration

v0.5.0 will introduce global Jolter configuration.

Examples:

```bash
jolter config set ...
jolter config get ...
jolter config list
```

The system will provide a familiar configuration experience similar to:

* Git
* npm
* Cargo

Global configuration will be designed to work alongside project-level configuration while keeping project requirements deterministic and explicit.

---

## Registry Architecture

The instruction registry will become the foundation of Jolter's ecosystem.

The registry will be designed around:

* versioned instructions
* deterministic resolution
* integrity verification
* platform-aware artifacts
* runtime and tool dependencies
* extensible instruction types
* local caching

The registry should contain declarative instructions rather than arbitrary executable code.

This allows Jolter to remain small while the ecosystem can grow independently of the Jolter binary.

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
