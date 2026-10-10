#!/usr/bin/env python3
"""Matched Linux/Xvfb terminal interaction measurements; not a superiority benchmark.

Measures native key event to first changed fixture pixel (includes sampling overhead),
output while typing, resize to fixture redraw, and UI/service idle resources. Startup
and reconnect distributions are measured separately by bench_desktop.py.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tempfile
import time

import bench_desktop as desktop
import bench_service as service
import smoke_gui as gui

COLORS = [0x2182a5, 0xc27531]
DONE = 0x7f33ab
RESIZED = 0xac398b
FIXTURE = r'''import os,select,signal,sys,threading,time,tty
from pathlib import Path
ready,stats=sys.argv[1:]
tty.setraw(0)
lock=threading.Lock()
def bar(color,row=1):
    r,g,b=(color>>16)&255,(color>>8)&255,color&255
    return (f'\x1b7\x1b[{row};1H\x1b[48;2;{r};{g};{b}m'+(' '*120)+'\x1b[0m\x1b8').encode()
def write(data):
    with lock:
        view=memoryview(data)
        while view:
            view=view[os.write(1,view):]
write(b'\x1b[2J\x1b[H\x1b[4;32r\x1b[4;1H'+bar(0x21a545))
Path(ready).write_text(str(os.getpid()))
def output():
    # GUI startup/resizes reset the terminal scroll region. Reapply it here so
    # output cannot scroll away the protected key-response marker between frames.
    write(b'\x1b[4;32r\x1b[4;1H'+bar(0x1f1f21,2))
    block=(b'\x1b[31m'+b'x'*80+b'\x1b[0m\r\n')*256
    size=0; started=time.perf_counter()
    for _ in range(512):
        write(block);size+=len(block)
        time.sleep(.001) # Let the fixture reader acquire its output lock fairly.
    write(bar(0x7f33ab,2))
    Path(stats).write_text(f'{size} {time.perf_counter()-started}')
def resized(*_):
    # SIGWINCH can interrupt a write; defer the response to avoid lock recursion.
    threading.Thread(target=lambda:write(bar(0xac398b,3)),daemon=True).start()
signal.signal(signal.SIGWINCH,resized)
count=0
while True:
    if select.select([0],[],[],1)[0]:
        for char in os.read(0,4096):
            if char==ord('b'): threading.Thread(target=output,daemon=True).start()
            elif char==ord('r'): write(bar(0x1f1f21,3))
            elif char==ord('a'):
                write(bar([0x2182a5,0xc27531][count%2]));count+=1
'''


def distribution(values):
    ordered = sorted(values)
    return {'samples': len(ordered), 'median_ms': round(statistics.median(ordered), 3),
            'p95_ms': round(ordered[max(0, math.ceil(.95 * len(ordered)) - 1)], 3),
            'maximum_ms': round(max(ordered), 3), 'samples_ms': [round(value, 3) for value in values]}


def visible_color(dpy, root, color):
    image = desktop.X.XGetImage(dpy, root, 100, 0, 1, 400, desktop.W(-1).value, 2)
    if not image: return False
    try:
        return any(desktop.X.XGetPixel(image, 0, row) & 0xffffff == color for row in range(0, 400, 2))
    finally:
        desktop.X.XDestroyImage(image)


def sample_idle(app_pid, service_pid, seconds):
    start = time.perf_counter()
    before = {pid: desktop.ticks(pid) for pid in (app_pid, service_pid) if pid is not None}
    hosted_before = service.resources(service_pid)['hosted']
    time.sleep(seconds)
    duration = time.perf_counter() - start
    result = {'seconds': round(duration, 3)}
    for label, pid in (('desktop', app_pid), ('service', service_pid)):
        if pid is not None:
            result[label] = {'rss_mib': round(desktop.rss(pid), 3),
                             'cpu_percent_one_core': round(100 * (desktop.ticks(pid) - before[pid]) / os.sysconf('SC_CLK_TCK') / duration, 3)}
    hosted = service.resources(service_pid)['hosted']
    result['hosted_fixture'] = {'rss_mib': round(hosted['rss_bytes'] / 1048576, 3),
                                'cpu_percent_one_core': round(100 * (hosted['cpu_s'] - hosted_before['cpu_s']) / duration, 3)}
    return result


def measure(binary, args):
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1100x760x24', '-nolisten', 'tcp'],
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    display = ':' + server.stdout.readline().decode().strip()
    dpy = gui.wait_until(lambda: desktop.X.XOpenDisplay(display.encode()))
    root = desktop.X.XDefaultRootWindow(dpy)
    app = peer = None
    with tempfile.TemporaryDirectory(prefix='opterm-interaction-bench-') as directory:
        folder = Path(directory)
        env = dict(os.environ, DISPLAY=display, LIBGL_ALWAYS_SOFTWARE='1', WINIT_UNIX_BACKEND='x11',
                   NO_AT_BRIDGE='1', DBUS_SESSION_BUS_ADDRESS='', OPTERM_STATE_DIR=str(folder / 'service'),
                   OPTERM_CONFIG_DIR=str(folder / 'settings'), OPTERM_RECOVERY_DIR=str(folder / 'recovery'),
                   XDG_CONFIG_HOME=str(folder / 'config'), XDG_CACHE_HOME=str(folder / 'cache'), SHELL='/bin/sh')
        fixture = folder / 'fixture.py'; fixture.write_text(FIXTURE)
        ready, stats = folder / 'ready', folder / 'stats'
        result = {'binary': str(binary), 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
        try:
            app = subprocess.Popen([str(binary), '--', '/usr/bin/python3', str(fixture), str(ready), str(stats)],
                                   env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            gui.wait_until(lambda: visible_color(dpy, root, desktop.COLOR), seconds=30)
            window = gui.wait_until(lambda: next(iter(desktop.windows(dpy, root)), None))
            gui.wait_until(lambda: 'Map State: IsViewable' in subprocess.check_output(['xwininfo', '-display', display, '-id', str(window)], text=True, stderr=subprocess.DEVNULL))
            desktop.X.XMoveWindow(dpy, window, 0, 0)
            desktop.X.XSetInputFocus(dpy, window, 1, 0); desktop.X.XFlush(dpy)
            time.sleep(.4)
            peer = service.Peer(folder / 'service')
            service_pid = peer.hello['pid']
            session = peer.call('List')['List'][0]['id']
            result['idle_attached'] = sample_idle(app.pid, service_pid, args.idle_seconds)
            sample_index = 0
            def key(char):
                code = desktop.X.XKeysymToKeycode(dpy, ord(char))
                gui.XT.XTestFakeKeyEvent(dpy, code, 1, 0); gui.XT.XTestFakeKeyEvent(dpy, code, 0, 0)
                desktop.X.XFlush(dpy)
            def latency():
                nonlocal sample_index
                target = COLORS[sample_index % 2]
                started = time.perf_counter(); key('a')
                deadline = started + 30
                while not visible_color(dpy, root, target):
                    assert time.perf_counter() < deadline, 'Key-to-pixel fixture timed out'
                    time.sleep(.002)
                sample_index += 1
                return 1000 * (time.perf_counter() - started)
            result['key_to_pixel_idle'] = distribution([latency() for _ in range(args.samples)])
            started = time.perf_counter(); key('b')
            loaded = []
            time.sleep(.03)
            while len(loaded) < args.samples and not visible_color(dpy, root, DONE):
                loaded.append(latency()); time.sleep(.03)
            gui.wait_until(lambda: visible_color(dpy, root, DONE), seconds=30)
            elapsed = time.perf_counter() - started
            gui.wait_until(stats.exists)
            payload, fixture_duration = stats.read_text().split()
            result['key_to_pixel_during_output'] = distribution(loaded) if loaded else {'samples': 0}
            result['output_while_typing'] = {'bytes': int(payload), 'visible_completion_ms': round(elapsed * 1000, 3),
                                            'visible_throughput_mib_s': round(int(payload) / 1048576 / elapsed, 3),
                                            'fixture_write_seconds': float(fixture_duration)}
            key('r'); gui.wait_until(lambda: not visible_color(dpy, root, RESIZED))
            before_resize = time.perf_counter()
            desktop.X.XResizeWindow(dpy, window, 900, 600); desktop.X.XFlush(dpy)
            gui.wait_until(lambda: visible_color(dpy, root, RESIZED))
            result['resize_to_fixture_pixel_ms'] = round(1000 * (time.perf_counter() - before_resize), 3)
            desktop.close_window(dpy, window); app.wait(timeout=10); app = None
            result['idle_detached'] = sample_idle(None, service_pid, args.idle_seconds)
            # Measure detached output into the independently owned live grid.
            peer.call({'Attach': session}); peer.call({'TakeControl': session})
            start = time.perf_counter()
            old_stats = stats.stat().st_mtime_ns
            peer.call({'Write': {'id': session, 'bytes': [ord('b')]}})
            peer.call({'Detach': session})
            gui.wait_until(lambda: stats.stat().st_mtime_ns != old_stats, seconds=30)
            # Reattach as observer only after the output fixture has written all data.
            peer.call({'Attach': session})
            gui.wait_until(lambda: any(cell.get('bg') == [0x7f, 0x33, 0xab]
                                      for cell in peer.call({'View': {'id': session, 'since': None}})['Snapshot']['screen']['cells']))
            elapsed = time.perf_counter() - start
            result['detached_output'] = {'bytes': int(payload), 'grid_completion_ms': round(elapsed * 1000, 3),
                                         'throughput_mib_s': round(int(payload) / 1048576 / elapsed, 3),
                                         'session_pid_retained': Path('/proc/' + ready.read_text()).exists()}
            return result
        except Exception:
            print('Interaction fixture failure screenshot:', gui.screenshot(display, 'interaction-bench-failure.png'))
            raise
        finally:
            if peer: peer.close()
            if app:
                app.terminate()
                try: app.wait(timeout=5)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
            subprocess.run([str(binary), '--stop-service'], env=env, timeout=10,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            desktop.X.XCloseDisplay(dpy)
            server.terminate(); server.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--candidate', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=31)
    parser.add_argument('--idle-seconds', type=float, default=3)
    parser.add_argument('--startup-repetitions', type=int, default=3)
    parser.add_argument('--output', type=Path, default=gui.TARGET / 'performance-priorities.json')
    args = parser.parse_args()
    assert args.samples >= 3 and args.idle_seconds > 0 and args.startup_repetitions >= 3
    result = {'platform': platform.platform(), 'renderer': 'Private Xvfb, LIBGL_ALWAYS_SOFTWARE=1; AccessKit inactive',
              'method': 'Matched fixed raw Python fixture; actual XTest key events to first sampled fixture-color pixel. Includes 2ms polling/XGetImage overhead; RSS is procfs not PSS/peak. OS caches warm; sequential baseline then candidate.',
              'measured_at_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}
    for label, path in (('baseline', args.baseline), ('candidate', args.candidate)):
        binary = path.resolve()
        print('Measuring', label, binary, flush=True)
        result[label] = measure(binary, args)
        startup_path = args.output.with_name(args.output.stem + '-' + label + '-startup.json')
        subprocess.run([sys.executable, str(Path(__file__).with_name('bench_desktop.py')), str(binary),
                        '--repetitions', str(args.startup_repetitions), '--idle-seconds', str(args.idle_seconds),
                        '--output', str(startup_path)], check=True, stdout=subprocess.DEVNULL,
                       env=dict(os.environ, NO_AT_BRIDGE='1', DBUS_SESSION_BUS_ADDRESS=''))
        result[label]['startup'] = json.loads(startup_path.read_text())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(args.output, flush=True)


if __name__ == '__main__':
    main()
