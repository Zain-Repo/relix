# Dashboard performance lab

Run the reproducible workload in a release build:

```powershell
cargo run --release --example dashboard_benchmark -- 10000
cargo run --release --example dashboard_benchmark -- 100000
cargo run --release --example dashboard_benchmark -- 1000000
```

The optional argument is the movement count (1 through 1,000,000); the default is
10,000. Every invocation creates an isolated temporary SQLite database. It accepts
no database path and never opens the application's ledger. The database and its
sidecar files are removed after connections close.

The dataset contains eight CAD registers and thirty business dates per register.
Each session is closed with its expected cash count. Movement amounts, directions,
reasons, and request keys are deterministic. Generation uses the normal database
API, including ledger constraints, balance triggers, transactions, and durability
settings. Only 1,000 generated movements are held in a batch at once.

The output separates generation time from three dashboard query times and checks
each snapshot's movement count and received/paid totals against independently
accumulated generation totals. A mismatch makes the process fail. Timings include
opening the dashboard connection and assembling the snapshot. They do not measure
chart rendering or UI frame latency.

The first query is the first dashboard invocation, **not** an operating-system
cold-cache measurement: generation has already touched the same file. Subsequent
queries can benefit from operating-system caching. Compare runs on the same
machine, power mode, storage device, release configuration, and workload size.
Build time is outside the reported timings.

For memory comparisons, build once and launch the resulting executable directly:

```powershell
cargo build --release --example dashboard_benchmark
.\target\release\examples\dashboard_benchmark.exe 1000000
```

Observe this executable's working set and peak working set in Task Manager or
another process profiler, rather than measuring Cargo and compiler processes.
SQLite caches and the operating system affect observed memory; bounded generation
batches alone are not evidence of a particular peak-memory limit. Record hardware,
entry count, peak memory, and all three query times alongside any performance
claim. This lab does not simulate simultaneous writes or guarantee customer-device
latency.

## Observed release run

Measured on October 9, 2026, using Windows 11 Pro, an Intel Core Ultra 7 155H
(16 cores, 22 logical processors), approximately 31.4 GiB usable physical memory,
and rustc 1.96.0. These are individual runs, not a statistical benchmark; other
development processes were active. Storage performance and system load can change
the results.

| Movements | Generation | Dashboard runs 1 / 2 / 3 | CLI peak working set |
| --- | --- | --- | --- |
| 10,000 | 0.973 s | 10.395 / 15.629 / 21.046 ms | Not measured |
| 100,000 | 2.275 s | 23.967 / 36.004 / 46.025 ms | 7.80 MiB |
| 1,000,000 | 17.602 s | 231.245 / 245.048 / 223.244 ms | 8.55 MiB |

Every query's movement count, session count and closed state, cash totals, net
flow, and absence of closing discrepancies matched the generated ledger. The
million-movement process exited successfully. Peak working set was read from the
isolated benchmark executable's Windows process counter; it is **not** a
measurement of the desktop application's memory or total operating-system file
cache. The benchmark generates its own file immediately before querying it, so
none of these query runs represents an operating-system cold cache.
