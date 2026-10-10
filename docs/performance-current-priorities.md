# Current priority performance measurements

The search, selection, session switching, Undo close, shell integration, copy mode,
accessibility, and readability changes have a matched Linux measurement harness.
These measurements identify a starting point for further optimization. They do
not establish superiority over iTerm2 or another terminal.

## Workload and boundaries

Measurements use release binaries on Linux x86_64, kernel 6.8.0-146-generic, glibc
2.39. The baseline is the installed Opterm binary preserved before this feature
batch. Each binary runs with separate configuration, recovery, service state, and
private Xvfb display. `LIBGL_ALWAYS_SOFTWARE=1` forces software rendering; AccessKit
is inactive for these performance runs. The native accessibility smoke test is
separate and enables the actual AT-SPI bridge.

The input fixture is a fixed raw-mode Python terminal process, avoiding shell
startup and completion variability. Actual XTest key events alternate a protected
colored row; XGetImage checks the first visible response at 2 ms polling intervals.
The recorded time includes X server and image-sampling overhead. During the output
case the fixture emits 11.375 MiB of ANSI-colored text while the harness continues
typing. Its 256-line chunks pause 1 ms to avoid fixture write-lock starvation; the
throughput is for this workload, not a maximum parser or transport rate.

Cold launch means a fresh service and UI with warm filesystem caches. Reconnection
uses the existing service and validates the original fixture PID. There are three
startup/reconnect repetitions and 31 idle input samples; output samples stop when
the burst completes. The resize measurement is one 900 × 600 resize followed by a
SIGWINCH response becoming visible. Resource samples cover three idle seconds;
CPU is expressed as a percentage of one core, and procfs RSS is neither PSS nor
peak memory. Detached throughput waits for the hosted process to finish writing,
then observes the completion marker in the independently owned terminal grid.

## Recorded results

The final comparison was recorded on 2026-10-10 UTC after the snapshot optimization.
Its exact baseline SHA-256 is `e45e0004267993ec6e65e121e7b013ab0207dd467db628e3a38fc33d8c61e499`; the measured candidate is
`ef6d7f3b1aa996e879649b465e18788f2b04995f4cadc98ce7239340b02af008`. Full samples are in
[the final raw measurements](performance-data/current-priorities-linux.json).

| Measurement | Preserved baseline | Candidate |
| --- | ---: | ---: |
| Shell ready, median of 3 | 182.8 ms | 191.0 ms |
| First visible output, median of 3 | 285.9 ms | 313.9 ms |
| Reconnected visible output, median of 3 | 183.7 ms | 186.2 ms |
| Idle input to pixel, median / p95 (sample count) | 30.3 / 38.4 ms (31) | 27.7 / 42.8 ms (31) |
| Input to pixel during output, median / p95 (sample count) | 35.4 / 40.2 ms (16) | 35.1 / 43.0 ms (15) |
| Visible output while typing, 11.375 MiB burst | 10.39 MiB/s | 11.09 MiB/s |
| Resize to visible fixture response, one sample | 50.6 ms | 102.9 ms |
| Attached desktop RSS / idle CPU | 161.42 MiB / 29.66% | 163.10 MiB / 32.32% |
| Attached service RSS / idle CPU | 8.50 MiB / 6.00% | 9.25 MiB / 5.00% |
| Detached service RSS / idle CPU | 9.00 MiB / 7.33% | 9.85 MiB / 5.66% |
| Detached grid output, 11.375 MiB burst | 15.68 MiB/s | 15.69 MiB/s |
| Detached hosted PID retained | Yes | Yes |

The hosted Python fixture used 11.63 MiB RSS and 0% idle CPU in both cases. Loaded
input medians were similar in this run; the candidate's p95 was slightly higher.
Both versions had substantial idle CPU under software rendering. Cold startup and
resize showed variation that needs more observations before optimization decisions.

The targeted optimization omits false `search_match`/`search_current` and absent
`hyperlink` metadata from ordinary cells in compact JSON snapshots. This removes
exactly 61 UTF-8 bytes per ordinary cell:

```text
,"search_match":false,"search_current":false,"hyperlink":null
```

Deserialization retains defaults, while active search flags and actual hyperlinks
remain present. This reduces repeated snapshot serialization, transfer, and parsing
work without changing terminal cell contents or active search/link behavior.

The [pre-optimization run](performance-data/current-priorities-linux-pre-sparse.json)
used candidate `1aa621662fa8bf436be00d917ada4117fd5307833a24f21d838281186f5af351`.
Its candidate loaded median/p95 was 39.4/79.3 ms and visible throughput 9.55 MiB/s;
the new artifact measured 35.1/43.0 ms and 11.09 MiB/s. These separately recorded
runs suggest the intended reduction in snapshot overhead, but do not isolate its
causal effect: baseline throughput also varied from 11.90 to 10.39 MiB/s.

An [earlier exploratory run](performance-data/current-priorities-linux-first-run.json)
also showed higher candidate latency under output. Across the three candidate
artifacts, individual resize observations were 154, 51, and 103 ms. They do not
establish a stable resize distribution. The exploratory startup subprocess
inherited the host DBus address with `NO_AT_BRIDGE=1`; later runs also empty that
address explicitly. A native accessibility smoke briefly overlapped initial
baseline startup in the pre-optimization run. No accessibility smoke overlapped
the final table's run.

Short sequential runs, a software renderer, background machine activity, and small
tail-sample counts limit the conclusions. With 15–16 loaded samples, nearest-rank
p95 is the maximum observation rather than a stable estimate of production tail
latency. The final baseline/candidate measurements ran in that fixed order; this
is neither a randomized comparison nor proof of performance superiority.

No real-GPU, screen-reader-active performance, native macOS/Windows, large restored
workspace, peak-memory, sustained agent workload, or competitor comparison is
included here.

## Reproduce

Preserve the actual baseline binary before rebuilding, then build the candidate:

```sh
cargo build --release --locked --bin opterm
python3 scripts/bench_interaction.py \
  --baseline target/perf-baseline-opterm \
  --candidate target/release/opterm \
  --samples 31 --idle-seconds 3 --startup-repetitions 3 \
  --output target/performance-current-priorities.json
```

The script needs Linux X11 libraries, Xvfb, `xwininfo`, and Python 3. It stops its
own isolated service after the run. Raw samples, UTC recording time, exact binary
SHA-256 hashes, attached/detached resources, fixture write duration, and separate
startup observations are retained in the JSON.

For native accessibility infrastructure coverage, run:

```sh
python3 scripts/smoke_accessibility.py --binary target/release/opterm
```

This requires system Python `pyatspi`/GObject bindings and Orca. It isolates the
DBus session and configuration, starts Orca with speech and braille disabled, and
checks native Terminal role, text, caret, focus, and selected word after a native
double click. That check passed on this Linux host. Audible/manual screen-reader
navigation and native macOS/Windows accessibility remain unverified.
