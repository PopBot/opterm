#!/usr/bin/env python3
"""Private Linux/X11 desktop benchmark; process-cold, OS caches remain warm.

Requires Xvfb and libX11. GNOME comparison also requires dbus-run-session.
Reports first fixture output actually visible, separately from shell readiness.
"""
import argparse
import ctypes as C
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
import statistics
import subprocess
import tempfile
import time

import smoke_gui as gui

X, D, W = gui.X, gui.D, gui.W
X.XGetImage.argtypes = [D, W, C.c_int, C.c_int, C.c_uint, C.c_uint, W, C.c_int]
X.XGetImage.restype = C.c_void_p
X.XGetPixel.argtypes = [C.c_void_p, C.c_int, C.c_int]
X.XGetPixel.restype = W
X.XDestroyImage.argtypes = [C.c_void_p]
COLOR = 0x21a545


def visible(display, root):
    image = X.XGetImage(display, root, 100, 0, 1, 400, W(-1).value, 2)
    if not image:
        return False
    try:
        return any(X.XGetPixel(image, 0, row) & 0xffffff == COLOR
                   for row in range(0, 400, 2))
    finally:
        X.XDestroyImage(image)


def windows(display, root):
    returned, parent, children, count = W(), W(), C.POINTER(W)(), C.c_uint()
    X.XQueryTree(display, root, C.byref(returned), C.byref(parent),
                 C.byref(children), C.byref(count))
    result = []
    try:
        for index in range(count.value):
            name = C.c_void_p()
            if X.XFetchName(display, children[index], C.byref(name)) and name:
                title = C.string_at(name).decode(errors='replace')
                X.XFree(name)
                if title.startswith(('Opterm', 'OPTERM_BENCH')):
                    result.append(children[index])
    finally:
        if children:
            X.XFree(children)
    return result


def close_window(display, window):
    event = gui.XEvent()
    event.message.type = 33
    event.message.display = display
    event.message.window = window
    event.message.message_type = X.XInternAtom(display, b'WM_PROTOCOLS', 0)
    event.message.format = 32
    event.message.data.longs[0] = X.XInternAtom(display, b'WM_DELETE_WINDOW', 0)
    X.XSendEvent(display, window, 0, 0, C.byref(event))
    X.XFlush(display)


def ticks(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return int(fields[11]) + int(fields[12])


def rss(pid):
    for line in Path(f'/proc/{pid}/status').read_text().splitlines():
        if line.startswith('VmRSS:'):
            return int(line.split()[1]) / 1024
    return 0


def children(pid):
    result = [pid]
    try:
        child_ids = set()
        for thread in Path(f'/proc/{pid}/task').iterdir():
            child_ids.update((thread / 'children').read_text().split())
        for child in child_ids:
            result.extend(children(int(child)))
    except FileNotFoundError:
        pass
    return result


def idle(process, seconds):
    time.sleep(1)
    ids = children(process.pid)
    initial = {pid: ticks(pid) for pid in ids}
    started = time.perf_counter()
    time.sleep(seconds)
    duration = time.perf_counter() - started
    return {'seconds': duration, 'processes': [
        {'pid': pid, 'command': Path(f'/proc/{pid}/comm').read_text().strip(),
         'rss_mib': round(rss(pid), 2),
         'cpu_percent_one_core': round(100 * (ticks(pid) - initial[pid]) /
                                       os.sysconf('SC_CLK_TCK') / duration, 2)}
        for pid in ids if Path(f'/proc/{pid}').exists()]}


def launch(binary, native, env, fixture, marker, display, root, warm=False):
    args = (['dbus-run-session', '--', 'gnome-terminal', '--wait',
             '--title=OPTERM_BENCH', '--hide-menubar', '--geometry=120x36',
             '--', '/bin/sh', '-c', fixture] if native else
            [str(binary)] + ([] if warm else ['--', '/bin/sh', '-c', fixture]))
    started = time.perf_counter()
    process = subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                               start_new_session=True)
    shell_ms = None
    deadline = started + 20
    while time.perf_counter() < deadline:
        if not warm and shell_ms is None and marker.exists():
            shell_ms = 1000 * (time.perf_counter() - started)
        if visible(display, root):
            return process, {'shell_ready_ms': shell_ms,
                             'visible_output_ms': 1000 * (time.perf_counter() - started)}
        assert process.poll() is None, ('Desktop exited before output', process.returncode)
        time.sleep(.005)
    raise RuntimeError('Fixture output never became visible')


def finish(process, display, root):
    for window in windows(display, root):
        close_window(display, window)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        process.wait(timeout=5)
    gui.wait_until(lambda: not visible(display, root))


def summary(values):
    return {'median': round(statistics.median(values), 2),
            'minimum': round(min(values), 2), 'maximum': round(max(values), 2)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('--repetitions', type=int, default=5)
    parser.add_argument('--idle-seconds', type=float, default=5)
    parser.add_argument('--native', action='store_true')
    parser.add_argument('--output', type=Path, default=gui.TARGET / 'bench-desktop.json')
    args = parser.parse_args()
    binary = args.binary.resolve()
    assert args.repetitions > 0 and args.idle_seconds > 0
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0',
                               '1100x760x24', '-nolisten', 'tcp'],
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    display = ':' + server.stdout.readline().decode().strip()
    dpy = gui.wait_until(lambda: X.XOpenDisplay(display.encode()))
    root = X.XDefaultRootWindow(dpy)
    result = {'platform': platform.platform(), 'binary': str(binary),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'measured_at_utc': datetime.now(timezone.utc).isoformat(),
              'renderer': 'Xvfb; LIBGL_ALWAYS_SOFTWARE=1', 'repetitions': args.repetitions,
              'opterm_cold': [], 'opterm_reconnect': [], 'gnome_cold': []}
    try:
        for native in ([False, True] if args.native else [False]):
            for repetition in range(args.repetitions):
                with tempfile.TemporaryDirectory(prefix='opterm-desktop-bench-') as directory:
                    folder = Path(directory)
                    env = dict(os.environ, DISPLAY=display, LIBGL_ALWAYS_SOFTWARE='1',
                               GTK_USE_PORTAL='0', GIO_USE_VFS='local', NO_AT_BRIDGE='1',
                               WINIT_UNIX_BACKEND='x11', XDG_CONFIG_HOME=str(folder / 'config'),
                               XDG_DATA_HOME=str(folder / 'data'), XDG_CACHE_HOME=str(folder / 'cache'),
                               XDG_RUNTIME_DIR=str(folder / 'runtime'),
                               OPTERM_STATE_DIR=str(folder / 'service'),
                               OPTERM_CONFIG_DIR=str(folder / 'settings'),
                               OPTERM_RECOVERY_DIR=str(folder / 'recovery'))
                    (folder / 'runtime').mkdir(mode=0o700)
                    marker = folder / 'shell-ready'
                    fixture = ("printf '\\033[2J\\033[H\\033[48;2;33;165;69m%120s\\033[0m\\n' BENCH; "
                               f"printf '%s' $$ > '{marker}'; exec /bin/sh")
                    process = None
                    try:
                        process, timing = launch(binary, native, env, fixture, marker, dpy, root)
                        result['gnome_cold' if native else 'opterm_cold'].append(timing)
                        if repetition == args.repetitions - 1 and not native:
                            result['opterm_idle'] = idle(process, args.idle_seconds)
                        finish(process, dpy, root)
                        process = None
                        if not native:
                            original_pid = marker.read_text()
                            original_mtime = marker.stat().st_mtime_ns
                            process, timing = launch(binary, False, env, fixture, marker, dpy, root, warm=True)
                            assert marker.read_text() == original_pid and marker.stat().st_mtime_ns == original_mtime
                            assert Path(f'/proc/{original_pid}').exists(), 'Original shell did not survive'
                            result['opterm_reconnect'].append(timing)
                            finish(process, dpy, root)
                            process = None
                    finally:
                        if process and process.poll() is None:
                            os.killpg(process.pid, signal.SIGTERM)
                            process.wait(timeout=5)
                        if not native:
                            subprocess.run([str(binary), '--stop-service'], env=env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
                        # A privately activated desktop portal may leave its
                        # disconnected FUSE mount after its private bus exits.
                        portal = folder / 'runtime' / 'doc'
                        if native:
                            subprocess.run(['fusermount3', '-uz', str(portal)],
                                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        result['summary_ms'] = {name: {
            key: summary([sample[key] for sample in samples if sample[key] is not None])
            for key in ('shell_ready_ms', 'visible_output_ms')
            if any(sample[key] is not None for sample in samples)}
            for name in ('opterm_cold', 'opterm_reconnect', 'gnome_cold')
            if (samples := result[name])}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps(result, indent=2))
    finally:
        X.XCloseDisplay(dpy)
        server.terminate()
        server.wait(timeout=5)


if __name__ == '__main__':
    main()
