#!/usr/bin/env python3
"""Build and measure RPC components and separate-process TCP round trips."""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import selectors
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def command(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, text=True, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--quick', action='store_true', help='smoke-sized samples')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/bench-report')
    parser.add_argument('--threads', type=int, default=2, help='Tokio workers per process')
    parser.add_argument('--bin-dir', type=Path, help='use prebuilt binaries; skip all builds/components')
    parser.add_argument('--source-sha', help='full source SHA when running a git archive on the cluster')
    args = parser.parse_args()
    if args.source_sha and not re.fullmatch(r'[0-9a-f]{40}', args.source_sha):
        parser.error('--source-sha must be a full commit SHA')
    if args.threads < 1:
        parser.error('--threads must be positive')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, TOKIO_WORKER_THREADS=str(args.threads))
    metadata = {
        'git_sha': args.source_sha or command(['git', 'rev-parse', 'HEAD'], capture_output=True).stdout.strip(),
        'platform': platform.platform(), 'machine': platform.machine(),
        'hostname': platform.node(),
        'cpu_count': os.cpu_count(), 'tokio_workers_per_process': args.threads,
        'toolchain': command(['cargo', '--version'], capture_output=True).stdout.strip(),
        'quick': args.quick, 'timestamp_unix': time.time(),
        'prebuilt_bin_dir': str(args.bin_dir) if args.bin_dir else None,
    }
    cpuinfo = Path('/proc/cpuinfo')
    if cpuinfo.exists():
        metadata['cpu_model'] = next((line.split(':', 1)[1].strip()
            for line in cpuinfo.read_text().splitlines() if line.startswith('model name')), 'unknown')
    (output / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    if args.bin_dir:
        binaries = args.bin_dir.resolve()
    else:
        with (output / 'primitives.json').open('w') as stdout:
            command(['cargo', 'bench', '--locked', '-p', 'rpc_driver', '--bench', 'primitives',
                     '--locked', '--', *(['--quick'] if args.quick else [])], stdout=stdout, env=env, timeout=600)
        command(['cargo', 'build', '--release', '--locked', '-p', 'rpc-smoke'], env=env, timeout=600)
        # Ask Cargo instead of assuming CARGO_TARGET_DIR is unset.
        cargo_metadata = json.loads(command(['cargo', 'metadata', '--no-deps', '--format-version', '1'],
                                            capture_output=True).stdout)
        binaries = Path(cargo_metadata['target_directory']) / 'release'

    results = []
    # Small sequential latency, pipelined throughput, larger payloads, and
    # multiple connections. Repeat the single automatic dispatch path.
    cases = [(1, 1, 64), (1, 32, 64), (1, 1, 4096), (1, 32, 4096), (4, 32, 64)]
    for connections, concurrency, payload in cases:
        for position in range(1, 5):
            label = f'c{connections}-q{concurrency}-b{payload}-run{position}'
            with (output / f'{label}.stderr.log').open('w') as stderr:
                server = subprocess.Popen([str(binaries / 'rpc-smoke-server'), '--port', '0'], stdout=subprocess.PIPE,
                    stderr=stderr, text=True, env=env)
                try:
                    with selectors.DefaultSelector() as selector:
                        selector.register(server.stdout, selectors.EVENT_READ)
                        if not selector.select(timeout=10):
                            raise RuntimeError('server did not become ready within 10 seconds')
                        line = server.stdout.readline().strip()
                    if not line.startswith('READY '):
                        raise RuntimeError(f'invalid server readiness: {line!r}')
                    run = command([str(binaries / 'rpc-bench-client'), '--addr', line[6:],
                        '--connections', str(connections), '--concurrency', str(concurrency),
                        '--payload-bytes', str(payload), '--iterations', '200' if args.quick else '5000',
                        '--warmup', '20' if args.quick else '1000'], env=env,
                        stdout=subprocess.PIPE, stderr=stderr, timeout=150)
                    row = json.loads(run.stdout)
                    if row['errors'] or row['completed'] != concurrency * (200 if args.quick else 5000):
                        raise RuntimeError(f'incomplete benchmark: {row}')
                    row.update(server_inline_ready_dispatch=True, run_position=position)
                    results.append(row)
                    (output / 'e2e.json').write_text(json.dumps({'metadata': metadata, 'runs': results}, indent=2) + '\n')
                    print(f'{label} automatic: {row["throughput_rps"]:.0f} RPC/s, '
                          f'p50={row["latency_us"]["p50"]:.1f} us, p99={row["latency_us"]["p99"]:.1f} us', flush=True)
                finally:
                    server.terminate()
                    try:
                        server.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        server.kill()
                        server.wait()
                    server.stdout.close()
    lines = ['| Connections | In flight | Bytes | Inline | Run | RPC/s | p50 µs | p99 µs |',
             '|---:|---:|---:|:---:|---:|---:|---:|---:|']
    for row in results:
        lines.append(f'| {row["connections"]} | {row["concurrency"]} | {row["payload_bytes"]} | '
                     f'{row["server_inline_ready_dispatch"]} | {row["run_position"]} | '
                     f'{row["throughput_rps"]:.0f} | {row["latency_us"]["p50"]:.1f} | {row["latency_us"]["p99"]:.1f} |')
    summary = '\n'.join(lines) + '\n'
    (output / 'summary.md').write_text(summary)
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as stream:
            stream.write(summary)


if __name__ == '__main__':
    main()
