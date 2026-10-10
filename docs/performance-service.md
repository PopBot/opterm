# Service and PTY measurements

The local release build reconnects a private session and returns its current screen in a median **7.27 ms**, and returns a parsed reply after input in **2.78 ms**. These are headless IPC measurements, not visible-window or physical keyboard latency. The measured optimization removes recording loss caused by small output fragments; it does not establish that Opterm is faster than another terminal.

## Reproduce

Run on native Linux with Python 3, `/bin/sh`, `awk`, and procfs:

```sh
cargo build --release --locked
python3 scripts/bench_service.py --binary target/release/opterm --output target/performance-service-linux.json
```

The script uses independent mode-0700 temporary service directories, launches only its own deterministic shells, and shuts down those services. It does not attach to existing work or load user shell startup files. Run it while other benchmarks and builds are stopped. Defaults are seven repetitions for startup/output, 31 for reconnect/input, and one bounded observation for each concurrency scenario. The JSON retains individual latency samples. This script exercises private development transport; it is not a supported public command API.

Recorded evidence: [before](performance-data/service-linux-before.json), [after](performance-data/service-linux-after.json). The final recording-on stress cases were repeated with an explicit asynchronous flush check; their timestamp is in the after JSON. Temporary service paths and terminal contents are not stored in these results.

## Environment and boundaries

Measurements on 2026-10-10 UTC used Linux Mint 22.3, kernel 6.8.0-146-generic, an AMD Ryzen 7 2700X (8 cores/16 threads), 62.7 GiB RAM, and local ext4 storage. Rust 1.98.0 built locked dependencies with the repository's release profile, thin LTO and one codegen unit. Python 3.12.3 drove `/bin/sh` (dash) and mawk 1.3.4. The executable includes the independent desktop title-cache correction, which does not run in service mode.

The final executable SHA-256 is `0ccccb1de738248ffdfe43cd701bd35936e3c45aa8dd609f61728ea4084e1663`. The after results identify revision `1db391c`; that checkpoint includes the ring-buffer correction. The harness's final timing/resource instrumentation was refined after that checkpoint; both the final script and raw evidence are included here.

Terminals are 100 columns by 32 rows, with 10,000 requested scrollback lines, capped at one million history cells. Recording uses the accepted defaults: 30 days, 5 GiB total and 256 MiB per stream. No font or renderer runs in this harness. Startup creates a fresh service process with a warm filesystem cache; it does not flush system caches or approximate first boot. A 0.5 ms polling sleep plus IPC/JSON decoding is included in input/output completion measurements. Native macOS and Windows tests passed in Phase 4, but their performance has not been measured here.

## Latency

| Boundary | Samples | Median ms | Observed p95 ms |
| --- | ---: | ---: | ---: |
| New service process to transport Hello | 7 | 13.83 | 15.83 |
| Configured service Create to ready shell screen | 7 | 6.19 | 8.77 |
| New service process through first ready shell screen | 7 | 19.98 | 24.74 |
| New connection, Hello and Attach to current screen | 31 | 7.27 | 8.86 |
| Input write to parsed reply in returned screen | 31 | 2.78 | 4.19 |

Reconnect retains the original service instance and shell PID marker. With seven observations, nearest-rank p95 is the maximum sample; it is not a stable production tail estimate. The fixture shell avoids user startup files and does no agent computation. [Desktop measurements](performance-desktop.md) cover the separate visible startup boundary.

## Output and recording

Each repetition generates 32,768 colored 80-character rows: 2,981,888 source bytes (2.844 MiB), plus a completion marker. The direct baseline runs the same generator and collects its stdout through a pipe. Opterm runs it in a PTY, interprets ANSI state and returns the final screen through IPC. Linux's default ONLCR expands CRLF to CRCRLF in the PTY, so the captured byte stream is about 1.1% larger. Throughput below uses the common source payload size. The direct pipe performs no terminal interpretation, recording or rendering; it establishes the generator/collection floor rather than a competing terminal score.

| Path | Samples | Median ms | Observed p95 ms | Source MiB/s |
| --- | ---: | ---: | ---: | ---: |
| Direct shell/awk to collected pipe | 7 | 15.92 | 22.28 | 178.6 |
| Opterm parsed screen, recording off | 7 | 116.71 | 138.66 | 24.37 |
| Opterm parsed screen, recording on | 7 | 123.82 | 161.67 | 22.97 |

Recording adds about 6.1% median wall time in this run. All seven recording-on outputs were captured: **21,102,774 bytes**, with no reported gap or write error. The recorder drains asynchronously; this is file capture after a flush wait, not an fsync or power-loss guarantee. Across the seven repetitions, service CPU was approximately 0.78 CPU-seconds without recording and 1.56 with recording; hosted shell/generator CPU was 0.75/0.79 CPU-seconds. Direct generator CPU was 0.113 CPU-seconds. These counters include kernel PTY work and waited child CPU where available, and are quantized by Linux clock ticks.

## Bounded concurrency

Each session runs a persistent shell, waits idle for one second, then produces 2,048 colored rows (182 KiB source payload). Input is sent to all sessions before waiting for their completion markers. One hundred sessions are a stress case, totaling 17.77 MiB source output; this is not a claim of a 100-session interactive desktop acceptance pass. CPU/resource sampling is outside the timed output interval. RSS is sampled at idle and after output completion, and does not measure a transient peak or physical memory ownership.

| Sessions | Recording | Output ms | Source MiB/s | Service idle RSS MiB | Service loaded RSS MiB | Hosted shell RSS MiB | Recording gaps |
| ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | Off | 38.18 | 4.65 | 5.38 | 12.67 | 1.62 | 0 |
| 8 | Off | 94.34 | 15.07 | 6.88 | 63.35 | 13.00 | 0 |
| 32 | Off | 286.28 | 19.87 | 12.75 | 237.96 | 52.00 | 0 |
| 100 | Off | 912.20 | 19.48 | 27.88 | 731.15 | 162.50 | 0 |
| 1 | On | 26.78 | 6.64 | 5.62 | 13.07 | 1.62 | 0 |
| 8 | On | 88.78 | 16.02 | 7.12 | 64.68 | 13.00 | 0 |
| 32 | On | 309.25 | 18.39 | 12.75 | 248.78 | 52.00 | 0 |
| 100 | On | 943.61 | 18.84 | 30.12 | 770.92 | 162.50 | 0 |

Exact persisted stress bytes were **188,464 / 1,507,712 / 6,030,871 / 18,846,492** at 1/8/32/100 sessions. Each matched its calculated raw PTY byte count after waiting up to three seconds for the asynchronous writer; no gap/error was reported. Recording off created no output files. Service idle CPU was approximately 0–1% of one core over the short one-second samples; hosted shells were below tick resolution. Loaded recording-on service CPU was 0.03/0.27/1.10/4.13 CPU-seconds respectively. RSS summed across processes can count shared pages more than once.

The substantial loaded memory is live scrollback, not an output database. A bounded queue/history cell count does not bound all process memory or aggregate memory across 128 allowed sessions. Larger/full scrollbacks can use more memory than this short workload. Concurrent agent computation, long-lived memory growth and saturation beyond 100 sessions remain unmeasured.

## Demonstrated correction and reference targets

The original writer queued 128 messages of at most 8 KiB. Small PTY reads exhausted those message slots before the intended 1 MiB byte capacity. The first matrix reported gaps in 3/15/42 streams at 8/32/100 sessions, although each stream emitted under 190 KiB. The replacement coalesces reads into a fixed-capacity 1 MiB byte ring, uses one bounded wake token, and drains at most 512 KiB per worker batch. No filesystem operation holds the capture-buffer lock. Capture never waits for disk or free capacity, and overflow still reports omitted bytes while live work continues.

A regression holds the disk lock, submits 4,096 fragments of 64 bytes, then verifies all 256 KiB persisted exactly with no gap. The same stress workloads now persist their full raw byte counts. The before harness undercounted hosted processes and sampled resources repeatedly inside the stress timing interval; those fields are explicitly marked in its JSON. Consequently, **before/after stress throughput is not an equivalent speed comparison**. Gap counts, the fragment regression and exact final capture demonstrate the intended reliability improvement.

These local reference thresholds leave margin around the first results and are useful regression alerts on this machine/workload, not cross-platform guarantees: first service/shell screen p95 below 40 ms; reconnect below 15 ms; input-to-parsed-screen below 8 ms; recorded single-stream output above 15 source MiB/s; zero recording gaps for the bounded matrix; and service RSS below 1 GiB after the 100-session short output workload. Full-scrollback aggregate memory needs a separate future budget.

No direct-command bookkeeping/API was added. WezTerm and Alacritty were unavailable locally and were not installed solely for this report. Native/default desktop terminal comparisons are reported separately where the startup harness can measure an equivalent boundary. Existing targeted Phase 1–4 tests cover detach/crash continuity, stale controls, storage failures, retention and queue overflow; this timing script does not rerun every failure scenario. No fastest-terminal or universal reliability claim follows from these measurements.
