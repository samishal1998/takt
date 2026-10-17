# Takt — Verified Technical Sources

**Checked:** 16 October 2026. These are primary platform/maintainer references used to constrain the proposal. They do not prove the proposed Takt interfaces are implemented. Moving documentation should be rechecked and exact dependency/toolchain versions recorded during M0. No lengthy quotations are reproduced.

<a id="s01"></a>
## S01 — concurrently capabilities

[Maintainer repository and API description](https://github.com/open-cli-tools/concurrently). Documents `maxProcesses`, success conditions, output handling, and configurable termination. This corrects an overly broad “only launches everything at once” comparison.

<a id="s02"></a>
## S02 — Node environment options

[Node.js command-line documentation](https://nodejs.org/api/cli.html#node_optionsoptions). `NODE_OPTIONS` configures Node invocations. A native supervisor does not eliminate its effect on inherited Node child environments.

<a id="s03"></a>
## S03 — Rust available parallelism

[std::thread::available_parallelism](https://doc.rust-lang.org/std/thread/fn.available_parallelism.html). Returns an estimate and documents host/container/affinity limitations. It does not expose current load.

<a id="s04"></a>
## S04 — Linux available-memory observation

[Linux man-pages: proc_meminfo(5)](https://man7.org/linux/man-pages/man5/proc_meminfo.5.html). Explains `MemAvailable` as an estimate for starting applications without swapping. A scheduling observation is not an allocation guarantee.

<a id="s05"></a>
## S05 — Linux cgroup v2 controllers

[Linux kernel cgroup-v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html). Defines CPU bandwidth, memory ceilings, group OOM behavior, event counters, swap controls, and hierarchy rules. The proposed owned-subtree layout and policy are Takt design choices, not copied interfaces.

<a id="s06"></a>
## S06 — Delegation and ownership

[systemd: Control Group APIs and Delegation](https://systemd.io/CGROUP_DELEGATION/). Describes delegated subtrees and ownership boundaries. Presence of cgroup files does not by itself establish permission to manage a subtree.

<a id="s07"></a>
## S07 — Apple resource limits, archival scope

[Apple archived getrlimit(2) reference](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/getrlimit.2.html). Describes process-specific resource limits. This archival reference is not a verified modern aggregate-memory isolation contract; native macOS behavior needs empirical validation.

<a id="s08"></a>
## S08 — rlimit distinctions

[Linux man-pages: getrlimit(2)](https://man7.org/linux/man-pages/man2/getrlimit.2.html). Distinguishes address-space, CPU-time, and other per-process limits. They are not interchangeable with aggregate workload memory and CPU-bandwidth policy.

<a id="s09"></a>
## S09 — UDP socket semantics

[Rust std::net::UdpSocket](https://doc.rust-lang.org/std/net/struct.UdpSocket.html). Defines UDP send/receive/peer behavior. Takt's validated response requirement is an application-level readiness design, not something implied by send success.

<a id="s10"></a>
## S10 — Unix child process setup

[Rust std::os::unix::process::CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html). Stable `process_group` is available. `pre_exec` requires careful unsafe/post-fork handling. Do not assume every method shown on the page is stable; verify the exact selected API.

<a id="s11"></a>
## S11 — Rust release-profile controls

[Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html). Documents optimization, stripping, LTO, codegen, and panic settings. A particular binary size remains a measurement, not a consequence guaranteed by choosing Rust.

<a id="s12"></a>
## S12 — Rust compilation targets

[Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html). Use to select candidate target triples and support expectations; artifact execution and OS/libc floor testing remain separate release requirements.

<a id="s13"></a>
## S13 — GitHub runner environments

[GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners). Describes labels, hardware variants, platform availability, and privilege context. Verify in the actual repository and record observed resources.

<a id="s14"></a>
## S14 — YAML parser candidate: yaml_serde

[yaml_serde maintainer API docs](https://docs.rs/yaml_serde/latest/yaml_serde/). Identifies a maintained YAML-organization fork and Serde integration. Candidate only; validate bounded parsing, diagnostics, features, and size.

<a id="s15"></a>
## S15 — YAML parser candidate: serde-saphyr

[serde-saphyr maintainer API docs](https://docs.rs/serde-saphyr/latest/serde_saphyr/). Alternative typed YAML implementation. Candidate only; package selection requires its own version/security/size review.

<a id="s16"></a>
## S16 — I/O candidate: Mio

[Mio Poll documentation](https://docs.rs/mio/latest/mio/struct.Poll.html). Documents readiness-based I/O and platform caveats. Takt must still supply correct process, timer, signal, and stream handling.

<a id="s17"></a>
## S17 — I/O candidate: polling

[polling documentation](https://docs.rs/polling/latest/polling/). Documents a portable OS event-polling interface. Candidate, not a guarantee of complete process-supervisor support.

<a id="s18"></a>
## S18 — GitHub Actions security

[GitHub secure-use reference](https://docs.github.com/en/actions/reference/security/secure-use). Supports least-privilege workflow design and full-length action commit pinning. Workflow SHAs must be resolved from the real action repository, not invented in a template.

## Brand/style references

[W3C WCAG 2.2 contrast minimum](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html) informs documentation text contrast. Logo colors are not a blanket palette for readable body text. [Inter's official site](https://rsms.me/inter/) is the typography reference. Font binaries are not included in this package.
