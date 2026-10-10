#!/usr/bin/env python3
"""Private Linux X11/AT-SPI check. Requires system Python pyatspi, Xvfb and Orca.

Checks native terminal text, focus and selection while Orca runs with speech/braille
disabled. This is infrastructure coverage, not a manual screen-reader usability review.
"""
import argparse
import ctypes as C
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

import smoke_gui as gui


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=gui.TARGET / 'debug/opterm')
    parser.add_argument('--in-private-bus', action='store_true')
    args = parser.parse_args()
    if not args.in_private_bus:
        # The bus launcher uses GSettings too. Isolate its configuration before
        # activating the private bus, not just the subsequently launched desktop.
        with tempfile.TemporaryDirectory(prefix='opterm-a11y-bus-') as directory:
            env = {**os.environ, 'XDG_CONFIG_HOME': directory + '/config',
                   'XDG_CACHE_HOME': directory + '/cache', 'XDG_DATA_HOME': directory + '/data'}
            return subprocess.call(['dbus-run-session', '--', '/usr/bin/python3', __file__,
                                    '--binary', str(args.binary.resolve()), '--in-private-bus'], env=env)
    import pyatspi
    from gi.repository import Gio, GLib
    bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    for setting in ('IsEnabled', 'ScreenReaderEnabled'):
        bus.call_sync('org.a11y.Bus', '/org/a11y/bus', 'org.freedesktop.DBus.Properties',
                      'Set', GLib.Variant('(ssv)', ('org.a11y.Status', setting, GLib.Variant('b', True))),
                      None, Gio.DBusCallFlags.NONE, 5000, None)
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1100x760x24', '-nolisten', 'tcp'],
                              stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    display = ':' + server.stdout.readline().decode().strip()
    dpy = gui.wait_until(lambda: gui.X.XOpenDisplay(display.encode()))
    root = gui.X.XDefaultRootWindow(dpy)
    app = orca = None
    with tempfile.TemporaryDirectory(prefix='opterm-accessibility-') as directory:
        folder = Path(directory)
        env = {**os.environ, 'DISPLAY': display, 'LIBGL_ALWAYS_SOFTWARE': '1', 'WINIT_UNIX_BACKEND': 'x11',
               'OPTERM_STATE_DIR': str(folder / 'runtime'), 'OPTERM_CONFIG_DIR': str(folder / 'settings'),
               'OPTERM_RECOVERY_DIR': str(folder / 'recovery'), 'XDG_CONFIG_HOME': str(folder / 'config'),
               'XDG_CACHE_HOME': str(folder / 'cache'), 'XDG_DATA_HOME': str(folder / 'data'), 'SHELL': '/bin/sh'}
        log = (folder / 'orca.log').open('w+')
        try:
            orca = subprocess.Popen(['orca', '--disable=speech', '--disable=braille', '--user-prefs', str(folder / 'orca')],
                                    env=env, stdout=log, stderr=log)
            app = subprocess.Popen([str(args.binary.resolve()), '--', '/bin/sh', '-c',
                                    "printf '\033[2J\033[HAccessibility fixture output\nsecond line\n'; exec /bin/sh"], env=env)
            from bench_desktop import windows
            window = gui.wait_until(lambda: next(iter(windows(dpy, root)), None))
            gui.wait_until(lambda: 'Map State: IsViewable' in subprocess.check_output(
                ['xwininfo', '-display', display, '-id', str(window)], text=True, stderr=subprocess.DEVNULL))
            gui.X.XMoveWindow(dpy, window, 0, 0)
            gui.X.XSetInputFocus(dpy, window, 1, 0); gui.X.XFlush(dpy)
            time.sleep(.4)
            def terminal():
                pending = [pyatspi.Registry.getDesktop(0)]
                count = 0
                while pending and count < 1024:
                    accessible = pending.pop()
                    count += 1
                    try:
                        if accessible.getRole() == pyatspi.ROLE_TERMINAL:
                            return accessible
                        pending.extend(accessible[index] for index in range(min(accessible.childCount, 128)))
                    except (RuntimeError, GLib.Error):
                        pass
                return None
            accessible = gui.wait_until(terminal, seconds=20)
            text = accessible.queryText()
            gui.wait_until(lambda: 'Accessibility fixture output' in text.getText(0, -1))
            assert 'second line' in text.getText(0, -1)
            assert text.caretOffset >= 0
            gui.wait_until(lambda: accessible.getState().contains(pyatspi.STATE_FOCUSED))
            # Select fixture by native double click, then read the platform text range.
            import bench_service
            observer = bench_service.Peer(folder / 'runtime')
            identifier = observer.call('List')['List'][0]['id']
            observer.call({'Attach': identifier})
            snapshot = observer.call({'View': {'id': identifier, 'since': None}})['Snapshot']
            fixture_row = next(cell['row'] for cell in snapshot['screen']['cells'] if cell['text'] == 'A')
            observer.close()
            gui.XT.XTestFakeMotionEvent(dpy, -1, round(6 + 17.4 * 8.43), round(36 + (fixture_row + .4) * 16.8), 0)
            for _ in range(2):
                gui.XT.XTestFakeButtonEvent(dpy, 1, 1, 0); gui.XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
                gui.X.XFlush(dpy); time.sleep(.07)
            def selected_word():
                if text.getNSelections() < 1: return None
                selection = text.getSelection(0)
                return text.getText(selection[0], selection[1])
            try:
                gui.wait_until(lambda: selected_word() == 'fixture')
            except RuntimeError:
                import bench_service
                observer = bench_service.Peer(folder / 'runtime')
                identifier = observer.call('List')['List'][0]['id']
                observer.call({'Attach': identifier})
                snapshot = observer.call({'View': {'id': identifier, 'since': None}})['Snapshot']
                print('Kernel selected text:', repr(snapshot['selected_text']))
                print('AT-SPI selected text:', repr(selected_word()))
                print('AT-SPI full text:', repr(text.getText(0, -1)[:200]))
                print('Accessibility failure screenshot:', gui.screenshot(display, 'accessibility-failure.png', window))
                observer.close()
                raise
            assert orca.poll() is None, 'Orca exited during the native accessibility fixture'
            print('Native AT-SPI: Terminal role, readable text, caret, terminal focus and selected word passed.')
            print('Orca stayed running on the private bus with speech/braille disabled; audible/manual navigation review remains unverified.')
        finally:
            if app:
                app.terminate()
                try: app.wait(timeout=5)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
            subprocess.run([str(args.binary.resolve()), '--stop-service'], env=env, timeout=10,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if orca:
                orca.terminate()
                try: orca.wait(timeout=5)
                except subprocess.TimeoutExpired: orca.kill(); orca.wait()
            log.seek(0)
            if sys.exc_info()[0] is not None:
                print('Private Orca diagnostics:', log.read()[-2000:])
            log.close()
            gui.X.XCloseDisplay(dpy)
            server.terminate(); server.wait(timeout=5)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
