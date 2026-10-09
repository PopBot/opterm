#!/usr/bin/env python3
"""Focused Linux X11 check: input, clipboard, and real close/crash continuity.

Uses a private Xvfb display. Requires Xvfb, xwd, libX11 and libXtst.
Does not touch the user's desktop or require Python packages.
"""
import ctypes as C
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[1]
TARGET = ROOT / 'target'
X = C.CDLL('libX11.so.6')
XT = C.CDLL('libXtst.so.6')
D = C.c_void_p
W = C.c_ulong
X.XOpenDisplay.argtypes = [C.c_char_p]
X.XOpenDisplay.restype = D
X.XDefaultRootWindow.argtypes = [D]
X.XDefaultRootWindow.restype = W
X.XQueryTree.argtypes = [D, W, C.POINTER(W), C.POINTER(W), C.POINTER(C.POINTER(W)), C.POINTER(C.c_uint)]
X.XFetchName.argtypes = [D, W, C.POINTER(C.c_void_p)]
X.XFree.argtypes = [C.c_void_p]
X.XKeysymToKeycode.argtypes = [D, W]
X.XKeysymToKeycode.restype = C.c_uint
X.XKeycodeToKeysym.argtypes = [D, C.c_uint, C.c_int]
X.XKeycodeToKeysym.restype = W
X.XSetInputFocus.argtypes = [D, W, C.c_int, W]
X.XMapWindow.argtypes = [D, W]
X.XSync.argtypes = [D, C.c_int]
X.XResizeWindow.argtypes = [D, W, C.c_uint, C.c_uint]
X.XFlush.argtypes = [D]
X.XCloseDisplay.argtypes = [D]
X.XInternAtom.argtypes = [D, C.c_char_p, C.c_int]
X.XInternAtom.restype = W
class MessageData(C.Union):
    _fields_ = [('longs', C.c_long * 5), ('bytes', C.c_char * 20)]
class ClientMessage(C.Structure):
    _fields_ = [('type', C.c_int), ('serial', W), ('send_event', C.c_int),
                ('display', D), ('window', W), ('message_type', W),
                ('format', C.c_int), ('data', MessageData)]
class XEvent(C.Union):
    _fields_ = [('message', ClientMessage), ('padding', C.c_long * 24)]
X.XSendEvent.argtypes = [D, W, C.c_int, C.c_long, C.POINTER(XEvent)]
XT.XTestFakeKeyEvent.argtypes = [D, C.c_uint, C.c_int, W]
XT.XTestFakeMotionEvent.argtypes = [D, C.c_int, C.c_int, C.c_int, W]
XT.XTestFakeButtonEvent.argtypes = [D, C.c_uint, C.c_int, W]


def wait_until(check, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError('GUI check timed out')


def screenshot(display):
    data = subprocess.check_output(['xwd', '-display', display, '-root', '-silent'])
    h = struct.unpack('>25I', data[:100])
    header, _, _, depth, width, height, _, byte_order, _, _, _, bits, stride, _, rm, gm, bm, _, _, colors, *_ = h
    assert depth == 24 and bits in (24, 32), (depth, bits)
    pixel_bytes = bits // 8
    pixels = data[header + colors * 12:]
    shifts = [(mask & -mask).bit_length() - 1 for mask in (rm, gm, bm)]
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            offset = y * stride + x * pixel_bytes
            pixel = int.from_bytes(pixels[offset:offset + pixel_bytes], 'little' if byte_order == 0 else 'big')
            rows.extend((pixel >> shift) & 255 for shift in shifts)
    def chunk(kind, body):
        return struct.pack('>I', len(body)) + kind + body + struct.pack('>I', zlib.crc32(kind + body))
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>2I5B', width, height, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')
    path = TARGET / 'prototype-linux.png'
    path.write_bytes(png)
    return path


def main():
    TARGET.mkdir(exist_ok=True)
    marker = TARGET / 'gui-input.txt'
    marker.unlink(missing_ok=True)
    server = subprocess.Popen(['Xvfb', '-displayfd', '1', '-screen', '0', '1100x760x24', '-nolisten', 'tcp'], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    state = tempfile.TemporaryDirectory(prefix='opterm-gui-')
    app = None
    dpy = None
    env = None
    binary = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else TARGET / 'debug/opterm'
    try:
        display = ':' + server.stdout.readline().decode().strip()
        env = {**os.environ, 'DISPLAY': display, 'LIBGL_ALWAYS_SOFTWARE': '1', 'WINIT_UNIX_BACKEND': 'x11', 'OPTERM_STATE_DIR': state.name}
        app = subprocess.Popen([str(binary), '--', '/bin/sh'], cwd=ROOT, env=env)
        dpy = wait_until(lambda: X.XOpenDisplay(display.encode()))
        root = X.XDefaultRootWindow(dpy)
        def find_window():
            root_return, parent, children, count = W(), W(), C.POINTER(W)(), C.c_uint()
            X.XQueryTree(dpy, root, C.byref(root_return), C.byref(parent), C.byref(children), C.byref(count))
            found = None
            for i in range(count.value):
                name = C.c_void_p()
                if X.XFetchName(dpy, children[i], C.byref(name)) and name:
                    title = C.string_at(name).decode(errors='replace')
                    X.XFree(name)
                    if title.startswith('Opterm'):
                        found = children[i]
            if children:
                X.XFree(children)
            return found
        window = wait_until(find_window)
        X.XMapWindow(dpy, window)
        X.XSync(dpy, 0)
        time.sleep(0.3)
        X.XSetInputFocus(dpy, window, 1, 0)
        X.XFlush(dpy)
        time.sleep(0.5)
        def key(symbol, pressed):
            code = X.XKeysymToKeycode(dpy, symbol)
            assert code, symbol
            XT.XTestFakeKeyEvent(dpy, code, pressed, 0)
        def type_text(text):
            for char in text:
                symbol = 0xff0d if char == '\n' else ord(char)
                code = X.XKeysymToKeycode(dpy, symbol)
                shifted = X.XKeycodeToKeysym(dpy, code, 0) != symbol
                if shifted:
                    key(0xffe1, 1)
                key(symbol, 1)
                key(symbol, 0)
                if shifted:
                    key(0xffe1, 0)
            X.XFlush(dpy)
        # New tab using the accepted Linux binding.
        key(0xffe3, 1); key(0xffe1, 1); key(ord('t'), 1); key(ord('t'), 0); key(0xffe1, 0); key(0xffe3, 0)
        X.XFlush(dpy)
        time.sleep(0.5)
        # Interrupt must reach the shell rather than copying the screen.
        type_text('sleep 30\n')
        time.sleep(0.2)
        key(0xffe3, 1); key(ord('c'), 1); key(ord('c'), 0); key(0xffe3, 0)
        X.XFlush(dpy)
        time.sleep(0.2)
        X.XResizeWindow(dpy, window, 1000, 680)
        X.XFlush(dpy)
        type_text("printf '\\033[2J\\033[H\\033[32mOpterm prototype\\033[0m\\n\\nA real shell in a thin native window.\\n\\n'; printf 'ANSI colors: \\033[31mred \\033[32mgreen \\033[34mblue\\033[0m\\n'; printf 'Terminal size: '; stty size; printf '\\n'; printf GUI_INPUT_OK > '" + str(marker) + "'\n")
        try:
            wait_until(lambda: marker.exists() and marker.read_text() == 'GUI_INPUT_OK')
        except RuntimeError:
            print('Failure screenshot:', screenshot(display))
            raise
        time.sleep(0.3)
        print('GUI input, new tab, Ctrl+C, and shell command: passed')
        print('Screenshot:', screenshot(display))
        # Round-trip a selected terminal word through the native clipboard.
        clipboard_marker = TARGET / 'gui-clipboard.txt'
        clipboard_marker.unlink(missing_ok=True)
        type_text("printf '\\033[2J\\033[Hprintf\\n'\n")
        time.sleep(0.3)
        XT.XTestFakeMotionEvent(dpy, -1, 7, 43, 0)
        XT.XTestFakeButtonEvent(dpy, 1, 1, 0)
        X.XFlush(dpy)
        time.sleep(0.15)
        XT.XTestFakeMotionEvent(dpy, -1, 53, 43, 0)
        X.XFlush(dpy)
        time.sleep(0.15)
        XT.XTestFakeButtonEvent(dpy, 1, 0, 0)
        X.XFlush(dpy)
        for letter in ('c', 'v'):
            key(0xffe3, 1); key(0xffe1, 1); key(ord(letter), 1); key(ord(letter), 0); key(0xffe1, 0); key(0xffe3, 0)
            X.XFlush(dpy)
            time.sleep(0.25)
        type_text(" CLIPBOARD_OK > '" + str(clipboard_marker) + "'\n")
        try:
            wait_until(lambda: clipboard_marker.exists() and clipboard_marker.read_text() == 'CLIPBOARD_OK')
        except RuntimeError:
            screenshot(display)
            raise
        print('Native selection, copy, and paste: passed')
        # Keep a shell variable/PID and background work across actual window exit.
        pid_marker = Path(state.name) / 'pid'
        work_marker = Path(state.name) / 'background'
        restored_marker = Path(state.name) / 'restored'
        type_text("export OPTERM_GUI=still-here; printf '%s' $$ > '" + str(pid_marker) + "'; (sleep 0.5; printf ALIVE > '" + str(work_marker) + "') &\n")
        wait_until(pid_marker.exists)
        original_pid = pid_marker.read_text()
        event = XEvent()
        event.message.type = 33  # ClientMessage, normal WM_DELETE_WINDOW request.
        event.message.display = dpy
        event.message.window = window
        event.message.message_type = X.XInternAtom(dpy, b'WM_PROTOCOLS', 0)
        event.message.format = 32
        event.message.data.longs[0] = X.XInternAtom(dpy, b'WM_DELETE_WINDOW', 0)
        X.XSendEvent(dpy, window, 0, 0, C.byref(event))
        X.XFlush(dpy)
        assert app.wait(timeout=5) == 0, 'Normal close failed'
        wait_until(lambda: work_marker.exists() and work_marker.read_text() == 'ALIVE')
        def reopen():
            process = subprocess.Popen([str(binary)], cwd=ROOT, env=env)
            new_window = wait_until(find_window)
            X.XMapWindow(dpy, new_window)
            X.XSync(dpy, 0)
            time.sleep(0.3)
            X.XSetInputFocus(dpy, new_window, 1, 0)
            X.XFlush(dpy)
            time.sleep(0.5)
            return process, new_window
        app, window = reopen()
        type_text("printf '%s:%s' $$ \"$OPTERM_GUI\" > '" + str(restored_marker) + "'\n")
        wait_until(lambda: restored_marker.exists() and restored_marker.read_text() == original_pid + ':still-here')
        print('Normal window close/reopen: same shell PID, variables, and background work passed')
        work_marker.unlink()
        ready = Path(state.name) / 'ready'
        type_text("(sleep 0.5; printf ALIVE > '" + str(work_marker) + "') & printf READY > '" + str(ready) + "'\n")
        wait_until(ready.exists)
        app.kill(); app.wait(timeout=5)
        wait_until(lambda: work_marker.exists() and work_marker.read_text() == 'ALIVE')
        restored_marker.unlink()
        app, window = reopen()
        type_text("printf '%s:%s' $$ \"$OPTERM_GUI\" > '" + str(restored_marker) + "'\n")
        wait_until(lambda: restored_marker.exists() and restored_marker.read_text() == original_pid + ':still-here')
        print('Desktop SIGKILL/reopen: same shell PID, variables, and background work passed')
    finally:
        if app:
            app.terminate()
            try:
                app.wait(timeout=3)
            except subprocess.TimeoutExpired:
                app.kill(); app.wait()
        if env:
            subprocess.run([str(binary), '--stop-service'], cwd=ROOT, env=env, timeout=5, check=True, stdout=subprocess.DEVNULL)
        state.cleanup()
        if dpy:
            X.XCloseDisplay(dpy)
        server.terminate(); server.wait(timeout=3)


if __name__ == '__main__':
    main()
