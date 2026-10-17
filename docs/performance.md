# Performance measurements

Performance numbers describe the measured executable, machine, and workload. The
initial goals are an executable below 1,000,000 bytes, supervisor quiescent RSS
below 5 MiB, and idle CPU below 0.1% of one core. These are recorded goals, not
correctness or release gates. No speed or memory superiority claim follows from
the implementation language.

Run the independent measurement script against a built native artifact:

```sh
python3 scripts/measure-performance.py \
  target/release/takt target/release/takt-test-fixture target/performance-evidence
```

The script requires only Python's standard library, the built fixture, and the
native executable. It creates no environment or package installation. A full
observation takes approximately 75 seconds. The longest fixture is independently
bounded to 75 seconds; finite cases have a 15-second outer deadline. At most four
workload tasks run concurrently, each allocation fixture uses 8 MiB, and output
is drained to the null device. Measurements use one CPU equivalent and 256 MiB
in admission budgets, and explicitly small task requests. The authoritative idle
observation uses default enforcement `auto` and records the actual capabilities,
including fallback watchdog cost. Finite workload comparisons use enforcement
`off` for equal conditions with the shell; those budgets do not claim OS
containment. Hard-limit verification uses the separate isolated safety suite.

`measurement.json` records the executable SHA-256, exact bytes, OS/kernel/libc,
observed logical CPUs/load, policies, raw observations, and median/min/max values.
Its measurements include:

- Seven samples after one warm-up for version startup and direct finite execution.
- A 60-second idle service observation sampled every 500 ms. Linux reads only the
  supervisor PID's `/proc` CPU ticks and RSS. macOS reads its native libproc task
  CPU totals and RSS, converting Mach ticks through the native timebase. Child workload usage is excluded. A separate sleeping
  fixture is sampled simultaneously as a noise reference.
- Eight identical fixed-iteration computation commands at one, two, and four
  active slots, comparing Takt with portable shell batches of the same commands
  and maximum concurrency. Neither execution path applies hard controllers.
- Separate mixed CPU/allocation/wait, sixteen tiny tasks, readiness plus finite
  dependent, and 1 MiB per-stream output workloads. These report Takt's observed
  behavior without asserting that a shell implements equivalent semantics.

The shell comparison measures overhead on a deliberately small fixture, not an
application benchmark. Warm caches, timer resolution, CPU throttling, host load,
shared libraries and CI contention affect results. Linux tick granularity can
quantize tiny CPU totals; a zero baseline does not prove a noise-free host. RSS
counts resident shared mappings and is not private memory. Keep the raw report
alongside any published chart or release claim.

For a quick script check, use `--idle-seconds 1 --samples 1`; the report explicitly
marks that it is not the required 60-second observation. Native macOS results
must come from macOS execution. A successful cross-check on Linux is not a
macOS performance measurement.

The macOS adapter follows Apple's [proc task information layout](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h)
and [Mach task accounting implementation](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c).

## Initial Linux VPS observation

The [raw report](evidence/linux-vps-performance.json) records the GNU release
executable measured on 2026-10-17, SHA-256
`8aa35a847a9f21d12fd88d5c9d44d4a7d8cb77daec4c78cab41e62b007621a75`.
It is an implementation checkpoint, not the final release artifact or a CI
measurement. The host ran Linux 6.8.0-79-generic, glibc 2.39, and eight logical
CPUs; starting load averages were 3.14, 4.90, and 4.68. Other processes were active
on this shared VPS. No native macOS result is implied.

| Measurement | Observed | Initial goal |
| --- | ---: | ---: |
| Executable size | 2,159,064 bytes | Below 1,000,000 bytes: missed |
| Supervisor idle RSS, median and maximum | 3.75 MiB | Below 5 MiB: met |
| Supervisor CPU over 60.0006 seconds | 2.18 CPU seconds; 3.63% of one core | Below 0.1%: missed |
| Sleeping fixture reference CPU | 0 observed CPU ticks | Noise reference only |
| Version startup, median [min, max] | 1.074 [1.048, 1.605] ms | No threshold |
| Direct finite execution, median [min, max] | 11.971 [11.491, 17.096] ms | No threshold |

Default `auto` enforcement used the fallback memory watchdog because this
invocation could not create a hard-protection subtree. Its process-group RSS
sampling scans visible `/proc` entries, so other processes on the VPS increase
observation work. That is a source of overhead in the implementation; this
measurement does not isolate how much of the CPU total it caused. The default
idle CPU and executable-size goals are unmet. Neither result is hidden by the
passing RSS goal or presented as an optimized result.

Eight identical fixed computation commands produced the following wall times.
The shell runs the same arguments with the same maximum active concurrency;
both paths use enforcement `off`. Values are medians [minimum, maximum] of seven
samples after a warm-up.

| Active slots | Takt (ms) | Portable shell (ms) |
| ---: | ---: | ---: |
| 1 | 100.469 [88.205, 121.454] | 30.001 [28.030, 35.561] |
| 2 | 78.322 [71.959, 96.174] | 17.731 [15.690, 21.881] |
| 4 | 70.262 [65.197, 87.685] | 9.131 [8.767, 10.957] |

Takt took longer than the shell for these small finite commands at every tested
concurrency. These numbers describe scheduling and process-management overhead
for this fixture and do not establish performance for larger application work.

| Additional Takt workload | Wall time, median [min, max] (ms) |
| --- | ---: |
| Mixed computation, 8 MiB allocation, and waiting | 124.104 [122.542, 135.434] |
| Sixteen tiny finite tasks | 144.601 [132.963, 155.268] |
| 1 MiB output on each stream | 16.463 [13.538, 17.788] |
| Ready service plus finite dependent | 25.253 [21.573, 26.858] |

## Repository check comparison

`scripts/measure-repository.py` compares the repository's format, lint, fixture
build, and test commands with a portable shell. The test command depends on a
successful fixture build in both paths. Two command slots are available; Cargo
itself uses two build jobs, tests use one test thread, and both paths inherit the
same environment and send both output streams to `/dev/null`. The generated
configuration and shell script are retained with the report.

Run this only after independently checking the source and warming its build
cache, with other builds stopped:

```sh
python3 scripts/measure-repository.py \
  target/release/takt target/repository-performance-evidence
```

Each runner receives one warm-up followed by three measured executions in
alternating order. Each execution has an independent 30-second deadline.
Enforcement is `off` for the comparison; explicit requests make two slots
available and are not OS memory limits. The script saves exit-checked wall
times, medians and spreads, the executable and input hashes, commands,
dependencies, and environment overrides. Cargo's shared cache and build lock
are part of this real repository workload. A small timing difference on this
shared host does not establish a speed advantage.

The [repository report](evidence/linux-vps-repository.json) records the same VPS
on 2026-10-17 using a later GNU executable, SHA-256
`00379e69ed00fac6739df3d3705ac2aa7edbe58e9fb71d9cdd274be03143e504`.
This checkpoint includes the process-sampling change for enforcement `off`;
the earlier default-`auto` idle observation above remains a separate measurement.
All eight executions, including warm-ups, completed successfully. Starting load
averages were 4.03, 3.77, and 4.09.

| Runner | Median wall time | Minimum | Maximum |
| --- | ---: | ---: | ---: |
| Takt, two slots | 9.825 s | 9.676 s | 10.387 s |
| Portable shell, two slots | 9.484 s | 9.425 s | 10.654 s |

The measured ranges overlap. Takt's median was about 0.34 seconds higher in
these three samples; this observation does not support a speed advantage.
