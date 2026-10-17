# Takt — Decision Record

**Date:** 16 October 2026

## Accepted from the discussion

Takt is the chosen name. Rust is the implementation direction after considering Bash and Go. Linux/macOS are first; Windows comes later. Finite CI/check commands are primary, with both sequential and parallel execution. User-supplied settings take priority over automatic discovery. Long-running services remain supported. Dependencies include finite-command exit codes and service readiness. Initial probes are TCP, HTTP, UDP, and stdout/stderr; custom commands are later. Small native delivery and CI-based proof matter. The selected brand is **Concept 02 Stack**, not Concept 01 Waveform and not the later hybrid.

The earlier binary-size ranges and idle-memory numbers were estimates, not measurements. The PRD treats below 1 MB as a target and requires actual measurements. Likewise, native macOS protection is not represented as Linux-equivalent hard containment.

## Proposed defaults, not owner-approved implementation facts

| Decision | Proposed default | Validation/approval needed |
|---|---|---|
| Invocation precedence | CLI → environment → file → default/auto | Confirm with example-based tests |
| Failure aggregation | Continue independent checks | Confirm UX; fail-fast remains explicit |
| Edge mismatch | Neutral skip when prerequisite otherwise succeeded | Confirm conditional branch reporting |
| Service exit after ready | Fail run, do not kill active dependents by default | Confirm optional stop-dependents scope |
| Unknown task request | 1 CPU + 1 GiB, inferred values reduced on smaller hosts | Benchmark real lint/build/test workloads |
| Auto memory reserve | max(256 MiB, 15% of initial headroom) | Validate constrained host behavior |
| Resource queue timeout | 120 seconds | Confirm default versus unlimited opt-in |
| Readiness timeout / retry / attempt | 30 s / 250 ms / 1 s | Test slow startup and network failure |
| Stop grace | 5 seconds | Test conventional child tools |
| Memory watchdog | 100–250 ms observation; explicitly best effort | Validate overhead and platform accounting |
| HTTP scheme scope | Plain HTTP in baseline V1 | Owner must decide whether HTTPS is V1-required |
| Small binary | Same V1 behavior, below 1,000,000 bytes when feasible | Measure before public promise |

## Decisions to close before implementation commits

Repository and software license are unspecified. No repository or issues were created. Confirm macOS/Linux compatibility floors and which architectures must ship on day one. Confirm the source/availability of a delegated Linux controller test environment. Decide HTTPS scope, exact CLI/schema surface, release artifact variants, and what size overage is acceptable with full functionality.

Do not block useful design work on trademark or package-name availability, but do not claim the name is legally cleared or a registry namespace is available. No such search or reservation is represented as completed.

## Brand production note

The kit's SVGs are clean vector reconstructions of the selected raster concept. The three offset horizontal bars and cyan-to-green direction are retained. The wordmark is standardized with outlined glyphs so it does not depend on an installed font. These are not original editable files recovered from the image generator. Original exploration boards are preserved separately and explicitly marked non-production references.

## Change procedure

When a tested API or OS behavior contradicts this specification: record the exact version/platform and reproduction, identify the affected promise, present the smallest viable alternatives, and obtain approval before changing scope or guarantees. Correctness and honest protection claims take priority over an arbitrary size target.
