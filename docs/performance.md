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
