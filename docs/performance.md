# Prototype performance and stack

Measurements are workload-specific. Opterm does not make the shell, compiler, or hosted AI model execute faster, and this prototype does not establish the fastest terminal on every operating system. The benchmark boundaries separate terminal/service overhead from program work.

## Reproducible measurements

See the [desktop report](performance-desktop.md) for launch/reconnection and desktop resources, and the [service report](performance-service.md) for PTY/transport/output workloads and concurrency. Each report records the environment, repetitions, workload, recording state, measurement boundary and raw-data reproduction command. Cross-platform build/PTY correctness evidence is recorded separately in [native validation](platform-validation.md).

## Local observations

| Boundary | Observed result |
| --- | --- |
| Fresh desktop, first output visible | 281 ms median, five repetitions |
| Reopened desktop, existing output visible | 155 ms median, five repetitions |
| Headless reconnect to current screen | 7.27 ms median / 8.86 ms observed p95, 31 samples |
| Input to parsed headless reply | 2.78 ms median / 4.19 ms observed p95, 31 samples |
| Single-stream ANSI output with recording | 22.97 source MiB/s median, seven repetitions |
| Bounded 1/8/32/100-session recording stress | Exact expected bytes captured; no gaps |

Visible timings use private X11 software rendering; headless timings include IPC and parsing but no window rendering. Small samples and warm filesystem caches limit inference. The reports preserve the direct-process floor and the non-equivalent fresh GNOME Terminal reference. WezTerm/Alacritty and physical Mac/Windows speed comparisons were unavailable.

## Demonstrated changes

An unchanged title command caused egui to request another frame indefinitely. Caching the last title removed that loop. The five-second software-rendered observation shows about 96% less idle desktop CPU with essentially unchanged RSS; small startup samples overlap and establish no startup improvement.

Fragmented output could fill the recording queue's message slots before its advertised byte capacity. A fixed-capacity byte ring now accepts up to one MiB regardless of fragment count, with one wake token and a separate maximum 512-KiB writer batch. It drops excess input explicitly and performs filesystem work outside the queue lock. A blocked-disk regression verifies that 4096 small fragments persist exactly without gaps.

## Stack decision

Retain Rust, portable-pty, alacritty_terminal and the eframe/egui OpenGL desktop for this prototype. They now support the accepted terminal controls and service-owned persistence, and native builds/PTY tests pass on both Mac architectures, Linux and Windows. This is a practical prototype choice; performance comparisons do not establish a final toolkit winner or justify a rewrite. No database, command ledger, public execution API or transcript index is added.

## Resource and correctness boundaries

Each desktop IPC client has a one-MiB queued-input bound; individual PTY input requests are also capped at one MiB. Recording has a separate one-MiB queue per session. Recording additionally enforces age, total and per-stream retention settings and reports lost recording bytes/errors while live PTYs continue. Scrollback is bounded by configured lines and one million cells per session, with the live grid separately capped. These bounds do not cap total RSS: fonts, Unicode extras, snapshots, threads and hosted programs also use memory.

Correctness evidence includes cancellation, stale controllers, failed writes, recording opt-out/gaps/retention, transport reconnection, desktop close/crash survival, missing-session restoration and settings recovery. See the tests and [focused desktop report](platform-native-checks.md). Reboot or service failure can end work; Opterm never replays commands to simulate survival.

Physical macOS/Windows desktop performance, native input-method composition, accessibility, mixed-DPI behavior and authenticated agent workloads remain unmeasured. The reports state unavailable or unequal competitor/baseline features explicitly. Unsigned prototype packages and their tested platform baselines are described in [distribution](distribution.md).

## Initial regression guide

These are loose investigation thresholds for the same Linux machine and exact fixtures, not production guarantees or portable CI timing assertions. Repeated excursions warrant investigation before a release:

| Metric | Prototype investigation threshold |
| --- | --- |
| Median fresh visible desktop output | 400 ms |
| Median existing desktop output after reconnect | 250 ms |
| Input-to-parsed-snapshot p95, headless fixture | 8 ms |
| Idle desktop, five seconds under software rendering | 60% of one CPU core |
| Desktop RSS with the measured fonts and one pane | 200 MiB |
| Service RSS after the bounded 100-session output fixture | 1 GiB, excluding hosted shells |

A recording gap is a correctness signal independent of latency: the bounded 182-KiB-per-stream stress fixture should capture every byte with no reported gap. Actual overflow and disk/retention failures must remain visible and must not freeze a PTY. The configured queue, screen and retention bounds remain in force on every host. Longer workloads and physical native desktop measurements are required before setting production budgets or claiming a validated minimum OS version.
