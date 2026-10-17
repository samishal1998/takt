<!-- Copy into the repository root after copying brand/ there. -->
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="brand/svg/takt-horizontal-on-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="brand/svg/takt-horizontal-on-light.svg">
  <img src="brand/svg/takt-horizontal-on-light.svg" alt="Takt" width="320">
</picture>

# Schedule. Balance. Flow.

A resource-aware command runner for finite checks and long-running services.

> Design-stage project. CLI examples are proposed until implemented and validated.

## Intended workflow

```sh
takt run checks
```

Independent tasks run together when admitted by the resource budget. Dependencies and readiness checks determine what is eligible; the remaining work waits with an explicit reason.

## Development priorities

Finite tasks first. Linux and macOS first. Explicit resource policy. Observable scheduling. Measured native-binary size.

<!-- Integration note: GitHub documents picture-based light/dark Markdown images at
https://github.blog/developer-skills/github/how-to-make-your-images-in-markdown-on-github-adjust-for-dark-mode-and-light-mode/
Verify the actual repository rendering, including any custom theme behavior. -->
