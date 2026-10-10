#!/usr/bin/env python3
"""Linux development benchmark; private fixtures, not a supported execution API."""
import argparse
import contextlib
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import resource
import socket
import statistics
import struct
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
OPTIONS = dict(enabled=False, retention_days=30, total_mib=5120,
               stream_mib=256, scrollback_lines=10000)
AWK = r'''BEGIN { for(i=0;i<n;i++) printf "\033[31m%08d%072d\033[0m\r\n", i, 0; printf "OUTPUT_DONE:%s\r\n", token }'''
FIXTURE = r'''stty -echo; printf 'READY_PID:%s\r\n' $$; while IFS= read -r line; do
case "$line" in out:*) arg=${line#out:}; n=${arg%%:*}; token=${arg#*:}; printf '\033[2J\033[H'; awk -v n="$n" -v token="$token" 'AWK';;
*) printf 'REPLY:%s\r\n' "$line";; esac; done'''.replace('AWK', AWK)


class Peer:
    def __init__(self, directory):
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.settimeout(20)
        self.socket.connect(str(directory / 'service.sock'))
        self.hello = self.call({'Hello': 1})['Hello']

    def receive(self, count):
        parts = bytearray()
        while len(parts) < count:
            block = self.socket.recv(count - len(parts))
            if not block:
                raise RuntimeError('Private benchmark service disconnected')
            parts.extend(block)
        return parts

    def call(self, request):
        data = json.dumps(request, separators=(',', ':')).encode()
        self.socket.sendall(struct.pack('<I', len(data)) + data)
        count, = struct.unpack('<I', self.receive(4))
        assert 0 < count <= 32 * 1024 * 1024
        reply = json.loads(self.receive(count))
        if isinstance(reply, dict) and 'Error' in reply:
            raise RuntimeError(reply['Error'])
        return reply

    def close(self):
        self.socket.close()


class Service:
    def __init__(self, binary, recording):
        self.temp = tempfile.TemporaryDirectory(prefix='opterm-bench-')
        self.directory = Path(self.temp.name)
        os.chmod(self.directory, 0o700)
        env = dict(os.environ, OPTERM_STATE_DIR=str(self.directory))
        self.started = time.perf_counter()
        self.process = subprocess.Popen([str(binary), '--session-service'], env=env,
                                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                        stderr=subprocess.DEVNULL, start_new_session=True)
        deadline = self.started + 20
        while True:
            try:
                self.peer = Peer(self.directory)
                break
            except (OSError, RuntimeError):
                assert self.process.poll() is None and time.perf_counter() < deadline
                time.sleep(.001)
        self.transport_ms = elapsed(self.started)
        self.peer.call({'ConfigHistory': dict(OPTIONS, enabled=recording)})

    def create(self):
        result = self.peer.call({'Create': ['/bin/sh', '-c', FIXTURE]})['Snapshot']
        identifier = result['info']['id']
        return identifier, self.wait(identifier, 'READY_PID:')

    def write(self, identifier, text):
        self.peer.call({'Write': dict(id=identifier, bytes=list(text.encode()))})

    def wait(self, identifier, marker):
        deadline = time.perf_counter() + 30
        while True:
            value = self.peer.call({'View': dict(id=identifier, since=None)})['Snapshot']
            if marker in screen_text(value):
                assert value['screen']['error'] is None
                return value
            assert time.perf_counter() < deadline, (marker, screen_text(value))
            time.sleep(.0005)

    def close(self):
        with contextlib.suppress(Exception):
            self.peer.call('Shutdown')
        with contextlib.suppress(Exception):
            self.peer.close()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.temp.cleanup()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def screen_text(snapshot):
    return ''.join(cell['text'] for cell in snapshot['screen']['cells'])


def elapsed(start):
    return (time.perf_counter() - start) * 1000


def distribution(values):
    ordered = sorted(values)
    return dict(n=len(values), median_ms=statistics.median(values),
                p95_ms=ordered[max(0, math.ceil(len(values) * .95) - 1)],
                min_ms=ordered[0], max_ms=ordered[-1], samples_ms=values)


def resources(pid):
    """Service and all hosted descendants separately; Linux procfs RSS is not PSS."""
    def read(process):
        try:
            fields = Path(f'/proc/{process}/stat').read_text().rsplit(')', 1)[1].split()
            children = {child for task in Path(f'/proc/{process}/task').iterdir()
                        for child in (task / 'children').read_text().split()}
            return dict(cpu_s=(int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK'),
                        waited_cpu_s=(int(fields[13]) + int(fields[14])) / os.sysconf('SC_CLK_TCK'),
                        rss_bytes=int(fields[21]) * os.sysconf('SC_PAGE_SIZE'),
                        threads=int(fields[17]), children=[int(child) for child in children])
        except (OSError, ValueError):
            return dict(cpu_s=0, waited_cpu_s=0, rss_bytes=0, threads=0, children=[])
    service = read(pid)
    pending = service.pop('children')
    hosted = dict(cpu_s=0, rss_bytes=0, threads=0, processes=0)
    while pending:
        item = read(pending.pop())
        pending.extend(item.pop('children'))
        hosted['processes'] += 1
        for key in ('cpu_s', 'rss_bytes', 'threads'):
            hosted[key] += item[key]
        hosted['cpu_s'] += item['waited_cpu_s']
    return dict(service=service, hosted=hosted)


def saved_bytes(directory):
    return sum(path.stat().st_size for path in (directory / 'history').glob('*.ansi'))


def latency(binary, repeats):
    starts, transports, shells = [], [], []
    for _ in range(repeats):
        with Service(binary, False) as service:
            transports.append(service.transport_ms)
            started = time.perf_counter()
            service.create()
            shells.append(elapsed(started))
            starts.append(elapsed(service.started))
    with Service(binary, False) as service:
        identifier, original = service.create()
        original_pid = re.search(r'READY_PID:\d+', screen_text(original)).group()
        instance = service.peer.hello['instance']
        attach, input_samples = [], []
        for index in range(31):
            service.peer.call({'Detach': identifier})
            service.peer.close()
            started = time.perf_counter()
            service.peer = Peer(service.directory)
            restored = service.peer.call({'Attach': identifier})['Snapshot']
            attach.append(elapsed(started))
            assert service.peer.hello['instance'] == instance
            assert original_pid in screen_text(restored)
            token = f'ping{index}'
            started = time.perf_counter()
            service.write(identifier, token + '\r')
            service.wait(identifier, 'REPLY:' + token)
            input_samples.append(elapsed(started))
    return dict(service_transport_start=distribution(transports),
                new_pty_ready=distribution(shells), new_service_and_pty_ready=distribution(starts),
                reconnect_and_attach_snapshot=distribution(attach),
                input_to_parsed_snapshot=distribution(input_samples))


def output(binary, repeats, lines):
    direct = []
    cpu_before = resource.getrusage(resource.RUSAGE_CHILDREN)
    for index in range(repeats):
        started = time.perf_counter()
        data = subprocess.check_output(['/bin/sh', '-c',
            "printf '\\033[2J\\033[H'; awk -v n=\"$1\" -v token=\"$2\" '" + AWK + "'",
            'opterm-bench', str(lines), f'direct{index}'])
        direct.append(elapsed(started))
        assert data.endswith(f'OUTPUT_DONE:direct{index}\r\n'.encode())
    cpu_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    result = dict(payload_lines=lines, payload_bytes=lines * 91,
                  direct_program_cpu_s=cpu_after.ru_utime + cpu_after.ru_stime - cpu_before.ru_utime - cpu_before.ru_stime,
                  direct_pipe_collect=distribution(direct))
    for recording in (False, True):
        with Service(binary, recording) as service:
            identifier, _ = service.create()
            timings, errors = [], []
            before = saved_bytes(service.directory)
            resource_before = resources(service.process.pid)
            for index in range(repeats):
                token = f'out{index}'
                started = time.perf_counter()
                service.write(identifier, f'out:{lines}:{token}\r')
                final = service.wait(identifier, 'OUTPUT_DONE:' + token)
                timings.append(elapsed(started))
                errors.append(final['info'].get('history_error'))
            resource_loaded = resources(service.process.pid)
            deadline = time.perf_counter() + 2
            previous = -1
            while time.perf_counter() < deadline:
                recorded = saved_bytes(service.directory)
                if recorded == previous:
                    break
                previous = recorded
                time.sleep(.1)
            stats = distribution(timings)
            stats['median_mib_s'] = lines * 91 / 1048576 / (stats['median_ms'] / 1000)
            stats.update(history_errors=errors, recorded_bytes=saved_bytes(service.directory) - before,
                         final_history_error=service.peer.call({'View': dict(id=identifier, since=None)})['Snapshot']['info'].get('history_error'),
                         resource_before=resource_before, resource_after=resource_loaded)
            stats['recorded_full_payload'] = stats['recorded_bytes'] >= lines * 91 * repeats if recording else None
            result['recording_on' if recording else 'recording_off'] = stats
            service.peer.call({'End': identifier})
    return result


def concurrency(binary, count, recording):
    with Service(binary, recording) as service:
        sessions, launch = [], []
        for _ in range(count):
            started = time.perf_counter()
            identifier, _ = service.create()
            sessions.append(identifier)
            launch.append(elapsed(started))
        idle_before = resources(service.process.pid)
        started = time.perf_counter()
        time.sleep(1)
        idle_seconds = time.perf_counter() - started
        idle_after = resources(service.process.pid)
        started = time.perf_counter()
        lines = 2048  # 182 KiB/session;100 stress remains under19 MiB of output.
        for identifier in sessions:
            service.write(identifier, f'out:{lines}:stress{identifier}\r')
        peak = idle_after
        errors = []
        for identifier in sessions:
            final = service.wait(identifier, f'OUTPUT_DONE:stress{identifier}')
            errors.append(final['info'].get('history_error'))
            current = resources(service.process.pid)
            if current['service']['rss_bytes'] > peak['service']['rss_bytes']:
                peak = current
        duration_ms = elapsed(started)
        loaded = resources(service.process.pid)
        assert len(service.peer.call('List')['List']) == count
        idle_cpu = {key: (idle_after[key]['cpu_s'] - idle_before[key]['cpu_s']) / idle_seconds * 100
                    for key in ('service', 'hosted')}
        loaded_cpu = {key: loaded[key]['cpu_s'] - idle_after[key]['cpu_s']
                      for key in ('service', 'hosted')}
        return dict(sessions=count, recording=recording, sequential_launch=distribution(launch),
                    idle=idle_after, idle_cpu_percent=idle_cpu, loaded=loaded,
                    sampled_peak=peak, output_ms=duration_ms, payload_bytes=count * lines * 91,
                    throughput_mib_s=count * lines * 91 / 1048576 / (duration_ms / 1000),
                    loaded_cpu_s=loaded_cpu, history_errors=errors,
                    recorded_bytes=saved_bytes(service.directory))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/release/opterm')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/performance-service.json')
    parser.add_argument('--repeats', type=int, default=7)
    parser.add_argument('--lines', type=int, default=32768)
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('Resource measurements currently require native Linux procfs')
    if not 3 <= args.repeats <= 15 or not 1024 <= args.lines <= 131072:
        parser.error('Use3–15 repetitions and1024–131072 output lines')
    binary = args.binary.resolve()
    result = dict(schema=1, timestamp_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                  git_revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  platform=platform.platform(), python=platform.python_version(), binary=str(binary),
                  binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                  shell='/bin/sh -c (no user startup files)', terminal=dict(columns=100, rows=32),
                  history_options=OPTIONS, polling_sleep_s=.0005,
                  cpu=Path('/proc/cpuinfo').read_text().split('model name\t: ')[1].splitlines()[0],
                  memory_total_kib=int(Path('/proc/meminfo').read_text().split('MemTotal:')[1].split()[0]))
    print('Latency', flush=True)
    result['latency'] = latency(binary, args.repeats)
    print('Output recording off/on and matched direct pipe', flush=True)
    result['output'] = output(binary, args.repeats, args.lines)
    result['concurrency'] = []
    for recording in (False, True):
        for count in (1, 8, 32, 100):
            print(f'Concurrency sessions={count} recording={recording}', flush=True)
            result['concurrency'].append(concurrency(binary, count, recording))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(args.output, flush=True)


if __name__ == '__main__':
    main()
