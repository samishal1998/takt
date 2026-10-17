# Releases and compatibility

Supported release targets are Linux x86-64 GNU (kernel 5.15+, glibc 2.35+), Linux
x86-64 musl (kernel 5.15+, no dynamic-loader or shared-library dependency), and
macOS Apple Silicon (macOS 15+). All three compile the same production features.
macOS uses system libraries; it is not described as a static binary. HTTPS,
Linux ARM64, macOS Intel, and Windows are deferred.

## Build and inspect

```sh
bash scripts/independent-checks.sh x86_64-unknown-linux-gnu
bash scripts/package.sh x86_64-unknown-linux-gnu
```

Packaging copies the executable, licenses, documentation, configuration schema,
and examples into a versioned archive. It extracts that archive and repeats the
independent black-box suite against the extracted executable. Fixture binaries
are retained as CI evidence only and never enter the release archive.

`scripts/measure-artifact.py` records executable bytes separately from archive
bytes, compiler/OS/target, SHA-256, and dependency evidence. GNU builds reject strong
undefined symbol or nonweak loader-version requirements newer than glibc 2.35;
optional weak imports are retained separately in the evidence. This distinction
matches glibc 2.35's [version loader](https://sourceware.org/git/?p=glibc.git;a=blob;f=elf/dl-version.c;hb=glibc-2.35)
and ELF's [weak symbol rules](https://gabi.xinuos.com/elf/05-symtab.html).
The actual baseline guest remains required to prove the fallback paths work.
Musl rejects an ELF interpreter or `NEEDED` entry;
macOS checks the deployment target and lists linked system libraries. The under
1,000,000-byte executable goal is reported without silently disabling features.
The reviewed initial baseline in `scripts/size-baseline.json` comes from the native
artifacts in [CI run 39049559556](https://github.com/samishal1998/takt/actions/runs/39049559556)
at commit `16c91e9347e217793f6f3304f2933cea06a62609`: GNU 2,159,032 bytes,
musl 2,282,336 bytes, and macOS 1,604,352 bytes. All production features are retained;
these exceed the size aspiration and are accepted under the approved nonblocking
size policy. This baseline records size, not acceptance of that run's failed safety
and compatibility harness gates. Subsequent growth
greater than the larger of 10% or 100 KiB fails packaging until its cause is reviewed
and the baseline is explicitly updated.

## Publish

The Release workflow accepts `vVERSION` tag pushes or a manual dispatch with a
`tag` input, for example `v0.1.0`. The tag must exactly match the package version
in `Cargo.toml`. A manual dispatch requires an unused tag and creates it at the selected workflow
commit when publication succeeds. Its reusable validation workflow must
finish successfully before publishing. This includes native checks, both Linux
controller lanes, extracted-artifact tests, and the actual Linux 5.15/glibc 2.35
guest. The publication step checks that all three archive names match the tag and
verifies their checksum files before creating the GitHub release. It attaches the
three archives, their individual checksum files, one `SHA256SUMS` manifest,
and per-target size and performance measurement JSON. Both release entry points
run the expanded 2,048-case property suite before publication.

After reviewing a commit, start the manual path with
`gh workflow run release.yml --ref main -f tag=v0.1.0`, substituting the intended
ref and package version. GitHub's workflow token creates the tag without starting
a second tag-push workflow. Existing tag pushes verify their tag before publishing.

Before creating a tag, review the complete handoff acceptance inventory and attach
evidence for cases beyond the automatic lanes described in [testing.md](testing.md).
A configured workflow is not a completed release gate. In particular, no native
macOS or baseline-VM execution result should be claimed from a Linux VPS build.
Unsigned macOS archives are not Apple-notarized installers; users should follow
their organization's policy for downloaded executables.

## Install a published archive

Download the archive matching your target and the release's `SHA256SUMS` file.
On Linux, verify the matching manifest entry with `sha256sum --check`; on macOS
use `shasum -a 256 --check`. Extract the archive and place `takt` in a directory
already on your `PATH`. Run `takt --version` and `takt doctor` to inspect the
installed executable and host capabilities. No package manager, Node runtime,
shell-piped installer, or additional probe executable is required.

Keep raw safety logs, controller snapshots, fixture outcomes, guest serial output,
and per-target measurements with the CI run. Startup, supervisor RSS, and idle CPU
goals require controlled measurements; archive size and successful compilation
are not substitutes for those measurements.
