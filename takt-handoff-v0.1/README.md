# Takt — Product and Brand Handoff

![Takt — Schedule. Balance. Flow.](brand/banners/takt-docs-banner-dark.png)

**Version:** 0.1 · **Prepared:** 16 October 2026

Takt is the proposed Rust-native, resource-aware command runner discussed in this session. Finite CI/check tasks are primary; dependency services and persistent concurrent commands are also included. Linux and macOS come first. The selected identity is **02 Stack**.

For a single-file handoff, use [the combined PRD and implementation Markdown](Takt-PRD-and-Implementation-Plan.md).

## Start here

| Document | Purpose |
|---|---|
| [Product requirements](docs/01-PRD.md) | Product scope, scheduling semantics, dependencies, readiness, limits, output, CLI and acceptance |
| [Implementation plan](docs/02-Implementation-plan.md) | Architecture, technical spikes, milestones, deliverables, backlog and agent instructions |
| [Test and CI plan](docs/03-Test-and-CI-plan.md) | Independent harness, bounded fixtures, native platform tests, real Linux safety tests and release gates |
| [Decision record](docs/04-Decisions-and-open-questions.md) | Accepted direction, proposed defaults and decisions still requiring approval |
| [Technical sources](docs/05-Technical-sources.md) | Primary documentation checked while preparing this package |
| [Configuration examples](examples/README.md) | Seven YAML/JSON examples with a draft schema |
| [Brand kit](brand/README.md) | SVG/PNG exports, icons, banners, CSS, tokens, style guide and original references |
| [Validation report](VALIDATION.md) | Checks actually performed on this package, and what was not tested |

## Important boundaries

This is a planning and design package, not an implemented Takt binary. CLI/configuration examples are proposed interfaces. No Takt repository has been specified, inspected, or modified. No application builds, tests, or GitHub Actions runs are claimed to have occurred.

The below-1 MB goal is a measurement target, not a demonstrated result. Resource requests are not hard limits. Linux hard protection depends on verified controller/delegation support; native macOS fallback must be described honestly. The agent must recheck APIs and report disagreements before changing accepted behavior.

The brand assets are standardized vector reconstructions of the selected Stack raster direction. Original boards, including rejected concepts, are preserved under `brand/reference/` for provenance. Wordmarks are outlined; no font binaries are included.

## Suggested review order

Review the PRD and decision record, close the repository/compatibility/HTTPS/size-policy questions, then hand the implementation and test plans to the coding agent. Open `brand/preview/index.html` or `brand/style/documentation-template.html` locally for the offline branding and documentation previews.

`MANIFEST.sha256` lists package-file integrity hashes. The separate brand archive is a convenience copy of this package's `brand/` directory.
