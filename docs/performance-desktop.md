# Desktop performance observations

Measured on 2026-10-09: Linux Mint 22.3, Linux 6.8.0-146, AMD Ryzen 7 2700X (8 cores / 16 threads). These are development measurements on a private Xvfb display with software rendering (`LIBGL_ALWAYS_SOFTWARE=1`), not hardware GPU or cross-platform acceptance results.

## Result and change

An idle terminal continuously repainted because `terminal()` sent the unchanged native window title every frame. In egui 0.33.3, `Context::send_viewport_cmd_to` requests an immediate repaint for every command. The next frame sent the same title again, creating a loop. Opterm now remembers the last title and sends a command only when it changes. Input, output polling, transfer polling, and settings polling intervals are unchanged.

In one five-second idle sample per build, desktop CPU fell from **859.79% to 34.80% of one core** (about 96% lower); RSS remained **161.45 → 161.42 MiB**. CPU above 100% means work across several cores, including software-renderer threads. The service measured **0.60 → 0.80%** CPU and **10.43 → 8.59 MiB** RSS; the fixture shell used 0% CPU and 1.82 MiB RSS. RSS includes shared mappings and is not exclusive physical memory. This short observation demonstrates removal of the repaint loop; it does not establish a production CPU budget. Periodic polling still redraws an idle window.

The extracted optimized release also passed `scripts/smoke_gui.py --controls-only`: literal search/navigation, tab rename/reorder, split metadata and transfers to new/existing windows retained the original shell PIDs and exactly four shells.

Startup did not improve measurably. Five repetitions per scenario gave these medians and full observed ranges:

| Scenario | Baseline median (range), ms | Title cache median (range), ms |
| --- | --- | --- |
| Fresh Opterm, shell fixture ready | 177.44 (170.40–179.15) | 186.90 (163.58–199.91) |
| Fresh Opterm, first fixture output visible | 259.34 (252.59–269.24) | 281.38 (250.39–286.28) |
| Reopen Opterm, existing output visible | 154.02 (135.49–163.64) | 155.36 (151.51–162.20) |

The ranges overlap. These small sequential batches support neither a startup improvement nor a reliable regression conclusion.

## Method and reproduction

Both artifacts used `cargo build --release --locked`, thin LTO and one codegen unit. The frozen baseline was built from `ae34f01`; the comparison changed only `src/main.rs` to cache the title. The independently owned later service-queue optimization is outside this desktop comparison. Exact artifact hashes, individual repetitions, and resource samples are retained in [desktop-baseline.json](performance-data/desktop-baseline.json) and [desktop-optimized.json](performance-data/desktop-optimized.json).

Run:

```sh
cargo build --release --locked
python3 scripts/bench_desktop.py target/release/opterm --repetitions 5 --native --output target/bench-desktop.json
```

Requirements: Python 3, Xvfb, libX11; the optional comparison requires GNOME Terminal, `dbus-run-session`, and `fusermount3`. The harness uses disposable settings/runtime/recovery directories and a private display. It does not launch into the user's desktop or change their settings. A private D-Bus isolates GNOME Terminal from the user's existing terminal server; optional GTK portal/VFS integration is disabled, and cleanup handles a private document-portal mount if one appears.

The launch fixture uses `/bin/sh -c` with no interactive shell configuration. It emits a known true-color background row, writes a PID marker, and executes `/bin/sh`. Shell readiness is the first observation of the marker. Visible readiness is the first matching pixel sampled from the private X root, including application initialization, service startup/attach and rendering. Sampling is approximately every 5 ms; this is not a keystroke-to-pixel latency benchmark.

Each fresh repetition has a fresh Opterm service and private directories. Reconnect closes the desktop normally, reopens its saved layout, verifies the original shell PID still exists and checks that the fixture marker was not rewritten. Commands are not replayed. OS page caches are not dropped, so “fresh” means new processes, not a cold disk cache. Builds and other benchmark stress were paused during the measurement windows. CPU is the change in `/proc` user/system ticks over five seconds after one second of settling; descendants are found across all process threads. CPU/resource sampling is one observation per build, not five independent resource trials.

## Native reference and limits

Installed GNOME Terminal 3.52.0 / VTE 0.76.0 ran the same shell fixture on the same private display, in five fresh D-Bus/server launches. Shell-ready median was **424.21 ms (382.17–498.26)**; visible-output median was **435.51 ms (393.14–509.02)**. This is a fresh-server reference only: everyday GNOME Terminal may reuse a running server and bus. Opterm and GNOME Terminal also use different renderer/font defaults, window sizing and persistence features. This result does not show that Opterm is generally faster than GNOME Terminal, the native macOS terminal, or Windows Terminal. The raw native resource entry contains only a partial launcher/bus process tree and is explicitly unsuitable for CPU/RSS comparison.

Remaining measurements include real GPU rendering, steady-state output throughput and latency, larger restored layouts, repeated memory/CPU samples, long-running agent workloads, and native macOS/Windows launch comparisons. [Service measurements](performance-service.md) separately cover service concurrency and output/control behavior. Native platform limitations remain in [platform-native-checks.md](platform-native-checks.md).
