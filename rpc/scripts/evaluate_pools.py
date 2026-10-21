#!/usr/bin/env python3
"""Measure the connection pools against each other on one host.

Runs the rpc-smoke echo workload through FixedSerialPool and MultiplexPool
at the same connection counts and in-flight depths, single-process and
multi-process, and reports throughput and latency per pool. Unlike
evaluate_runtimes.py this compares pools within one source revision, so it
needs no baseline build.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
POOLS = ['fixed', 'multiplex']
# (connections, calls in flight): the gateway's shape today, the same
# connections under load, and fewer connections carrying the same load.
SHAPES = [(8, 8), (8, 64), (8, 512), (16, 512), (2, 64)]


def run(argv, **kwargs):
    try:
        return subprocess.run(argv, cwd=ROOT, check=True, text=True, **kwargs)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        log = kwargs.get('stderr')
        if hasattr(log, 'name'):
            log.flush()
            print(Path(log.name).read_text()[-12000:], file=sys.stderr)
        raise


def smoke(binary, workers, arguments, duration, log):
    env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers))
    result = run([str(binary), '--skip-build', '--json', '--duration', str(duration), *arguments],
                 env=env, stdout=subprocess.PIPE, stderr=log, timeout=600)
    rows = json.loads(result.stdout)
    if not rows or any(r['total_errors'] or r['rpc_counts']['outstanding']
                       or not r['rpc_counts']['completed'] for r in rows):
        raise RuntimeError(f'failed pool workload: {arguments}')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--quick', action='store_true', help='one short repetition per case')
    parser.add_argument('--bin-dir', type=Path, help='use prebuilt rpc-smoke binaries; skip the build')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if args.bin_dir:
        binary = args.bin_dir / 'rpc-smoke'
    else:
        target = output / 'target'
        with (output / 'build.log').open('w') as log:
            run(['cargo', 'build', '--locked', '--release', '-p', 'rpc-smoke'],
                env=dict(os.environ, CARGO_TARGET_DIR=str(target)), stdout=log, stderr=log, timeout=900)
        binary = target / 'release/rpc-smoke'
    duration = 1 if args.quick else 5
    repetitions = 1 if args.quick else 2
    data = {'metadata': {
        'sha': run(['git', 'rev-parse', 'HEAD'], capture_output=True).stdout.strip(),
        'hostname': platform.node(), 'cpu_count': os.cpu_count(),
        'affinity': sorted(os.sched_getaffinity(0)),
        'toolchain': run(['rustc', '--version'], capture_output=True).stdout.strip(),
        'quick': args.quick, 'duration_secs': duration, 'repetitions': repetitions,
        'started_unix': time.time(),
        'notes': 'Echo profile, 64-byte payload, server and client in separate processes on '
                 'the same host with TOKIO_WORKER_THREADS workers each. Pool order alternates '
                 'between repetitions. The multi-process rows are the harness throughput sweep: '
                 'N client processes, 10 connections and 1000 calls in flight each.'},
        'single': [], 'throughput': []}

    def save():
        (output / 'pools.json').write_text(json.dumps(data, indent=2) + '\n')

    for workers in [2, 8]:
        for repeat in range(repetitions):
            pools = POOLS if repeat % 2 == 0 else list(reversed(POOLS))
            for connections, in_flight in SHAPES:
                for pool in pools:
                    label = f'single-w{workers}-{pool}-c{connections}-q{in_flight}-r{repeat}'
                    with (output / f'{label}.log').open('w') as log:
                        rows = smoke(binary, workers,
                                     ['single', 'echo', '--connections', str(connections),
                                      '--max-inflight', str(in_flight), '--pool', pool], duration, log)
                    row = rows[0]
                    data['single'].append(dict(workers=workers, repeat=repeat, pool=pool,
                                               connections=connections, in_flight=in_flight,
                                               throughput_rps=row['throughput_rps'],
                                               latency_us=row['latency_us']))
                    save()
                    print(f'{label}: {row["throughput_rps"]:.0f} RPC/s, '
                          f'p50 {row["latency_us"]["p50"]:.0f} µs', flush=True)
    for pool in POOLS:
        label = f'throughput-w8-{pool}'
        with (output / f'{label}.log').open('w') as log:
            rows = smoke(binary, 8, ['throughput', '--pool', pool], duration, log)
        for row in rows:
            data['throughput'].append(dict(workers=8, pool=pool, profile=row['profile_name'],
                                           connections=row['connections'],
                                           throughput_rps=row['throughput_rps'],
                                           latency_us=row['latency_us']))
            print(f'{label} {row["profile_name"]}: {row["throughput_rps"]:.0f} RPC/s aggregate',
                  flush=True)
        save()
    data['complete'] = True
    save()

    lines = ['', '## Single client process (median over repetitions)', '',
             '| Workers | Conns | In flight | Pool | RPC/s | p50 µs | p99 µs |',
             '|---|---|---|---|---|---|---|']
    for workers in [2, 8]:
        for connections, in_flight in SHAPES:
            for pool in POOLS:
                rows = [r for r in data['single'] if r['workers'] == workers and r['pool'] == pool
                        and r['connections'] == connections and r['in_flight'] == in_flight]
                lines.append(f'| {workers} | {connections} | {in_flight} | {pool} | '
                             f'{statistics.median(r["throughput_rps"] for r in rows):.0f} | '
                             f'{statistics.median(r["latency_us"]["p50"] for r in rows):.0f} | '
                             f'{statistics.median(r["latency_us"]["p99"] for r in rows):.0f} |')
    lines += ['', '## Multi-process throughput sweep (8 workers per process)', '',
              '| Clients × conns | Pool | Aggregate RPC/s | p50 µs | p99 µs |', '|---|---|---|---|---|']
    for row in data['throughput']:
        lines.append(f'| {row["profile"]} | {row["pool"]} | {row["throughput_rps"]:.0f} | '
                     f'{row["latency_us"]["p50"]:.0f} | {row["latency_us"]["p99"]:.0f} |')
    summary = '\n'.join(lines) + '\n'
    (output / 'pools.md').write_text(summary)
    print(summary, flush=True)


if __name__ == '__main__':
    main()
