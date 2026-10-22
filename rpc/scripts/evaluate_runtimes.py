#!/usr/bin/env python3
"""Compare main and the candidate with identical workloads and fixed CPU budgets.

Runs all rpc_bench scenarios plus separate-process TCP measurements. Runtime
partitioning is confined to benchmark executables; the driver has one policy.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import sys
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def run(argv, root=ROOT, **kwargs):
    try:
        return subprocess.run(argv, cwd=root, check=True, text=True, **kwargs)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
        log = kwargs.get('stderr')
        if hasattr(log, 'name'):
            log.flush()
            print(Path(log.name).read_text()[-12000:], file=sys.stderr)
        raise


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def resolve_baseline(requested):
    """The revision to compare against: `requested` if given, else where this
    checkout's history meets origin's main, so a branch is measured against
    the main it was built on rather than a pinned commit that goes stale. On
    main itself that is HEAD, so compare against its parent instead."""
    if requested:
        # Any local revision (HEAD^, a short SHA, a branch) resolves directly;
        # fetch only a commit this checkout doesn't have yet.
        local = subprocess.run(['git', 'rev-parse', '--verify', '--quiet', f'{requested}^{{commit}}'],
                               cwd=ROOT, text=True, capture_output=True)
        if local.returncode == 0:
            return local.stdout.strip()
        run(['git', 'fetch', '--quiet', '--depth=1', 'origin', requested])
        return run(['git', 'rev-parse', 'FETCH_HEAD'], capture_output=True).stdout.strip()
    shallow = run(['git', 'rev-parse', '--is-shallow-repository'],
                  capture_output=True).stdout.strip() == 'true'
    # Shallow CI checkouts carry no history to find the fork point in.
    run(['git', 'fetch', '--quiet', *(['--unshallow'] if shallow else []), 'origin', 'main'])
    head = run(['git', 'rev-parse', 'HEAD'], capture_output=True).stdout.strip()
    base = run(['git', 'merge-base', 'HEAD', 'FETCH_HEAD'], capture_output=True).stdout.strip()
    if base == head:
        base = run(['git', 'rev-parse', 'HEAD^'], capture_output=True).stdout.strip()
    return base


def build(root, target, log):
    env = dict(os.environ, CARGO_TARGET_DIR=str(target))
    # Ask Cargo for the exact executable, not an arbitrary matching stale file.
    result = run(['cargo', 'bench', '--locked', '-p', 'thruput-rpc-driver', '--bench',
                  'rpc_bench', '--bench', 'primitives', '--no-run', '--message-format=json'],
                 root=root, env=env, stdout=subprocess.PIPE, stderr=log, timeout=900)
    bench = next(Path(row['executable']) for row in map(json.loads, result.stdout.splitlines())
                 if row.get('executable') and row.get('target', {}).get('name') == 'rpc_bench')
    primitives = next(Path(row['executable']) for row in map(json.loads, result.stdout.splitlines())
                      if row.get('executable') and row.get('target', {}).get('name') == 'primitives')
    run(['cargo', 'build', '--locked', '--release', '-p', 'rpc-smoke'],
        root=root, env=env, stdout=log, stderr=log, timeout=900)
    return {'suite': bench, 'primitives': primitives, 'client': target / 'release/rpc-bench-client',
            'server': target / 'release/rpc-smoke-server',
            'smoke': target / 'release/rpc-smoke', 'smoke_client': target / 'release/rpc-smoke-client'}


def e2e(binaries, workers, shards, case, quick, log):
    connections, concurrency, payload = case
    iterations, warmup = (100, 20) if quick else (5000, 1000)
    env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers))
    server = subprocess.Popen([str(binaries['server']), '--port', '0',
                               '--runtime-shards', str(shards)], env=env,
                              stdout=subprocess.PIPE, stderr=log, text=True)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(server.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=15):
                raise RuntimeError('server readiness timed out')
            line = server.stdout.readline().strip()
        if not line.startswith('READY ') or len(line[6:].split(',')) != shards:
            raise RuntimeError(f'invalid readiness: {line!r}')
        result = run([str(binaries['client']), '--addr', line[6:],
                      '--runtime-shards', str(shards), '--connections', str(connections),
                      '--concurrency', str(concurrency), '--payload-bytes', str(payload),
                      '--iterations', str(iterations), '--warmup', str(warmup)],
                     env=env, stdout=subprocess.PIPE, stderr=log, timeout=150)
        row = json.loads(result.stdout)
        if row['errors'] or row['completed'] != concurrency * iterations:
            raise RuntimeError(f'incomplete measurement: {row}')
        return row
    finally:
        server.terminate()
        try:
            server.wait(timeout=5)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait()
        server.stdout.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--quick', action='store_true')
    parser.add_argument('--smoke-only', action='store_true', help='write a separate smoke-evaluation.json report')
    parser.add_argument('--baseline', help='revision to compare against (default: where HEAD forked '
                        "from origin's main, or HEAD's parent on main)")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work = output / 'builds'
    work.mkdir(exist_ok=True)
    worktree = work / 'main'
    baseline_sha = resolve_baseline(args.baseline)
    run(['git', 'worktree', 'add', '--detach', str(worktree), baseline_sha])
    # The worktree holds the whole repository; the RPC workspace members sit
    # at the same path inside it as ROOT does inside this checkout.
    toplevel = run(['git', 'rev-parse', '--show-toplevel'], capture_output=True).stdout.strip()
    baseline = worktree / ROOT.relative_to(Path(toplevel).resolve())
    try:
        # Share the workload and runtime harness, but keep each revision's
        # service implementations (everything above the marker): if the
        # candidate changes the handler API, its handlers would not compile
        # against the baseline driver, and a compatibility shim would distort
        # the result. Without an API change the two prefixes are identical.
        for name in ['rpc_driver/benches/rpc_bench.rs', 'rpc-smoke/src/server.rs',
                     'rpc-smoke/src/bench_client.rs', 'rpc-smoke/src/runtime.rs',
                     'rpc-smoke/src/harness.rs', 'rpc-smoke/src/client.rs',
                     'rpc-smoke/src/profile.rs', 'rpc-smoke/src/metrics.rs']:
            text = (ROOT / name).read_text()
            marker = {'rpc_driver/benches/rpc_bench.rs': '// Setup helpers',
                      'rpc-smoke/src/server.rs': '#[derive(Parser)]'}.get(name)
            if marker:
                original = (baseline / name).read_text()
                text = original[:original.index(marker)] + text[text.index(marker):]
            (baseline / name).write_text(text)
        with (output / 'build.log').open('w') as log:
            binaries = {
                'main': build(baseline, work / 'main-target', log),
                'candidate': build(ROOT, work / 'candidate-target', log),
            }
        hashes = {variant: {name: digest(path) for name, path in bins.items()}
                  for variant, bins in binaries.items()}
        # A change need not reach every executable (a read-loop change leaves
        # the primitives bench alone, a pool change the server), so only
        # refuse when nothing the evaluation runs differs from the baseline.
        if all(hashes['main'][key] == hashes['candidate'][key] for key in hashes['main']):
            raise RuntimeError(f'the candidate builds the same executables as baseline {baseline_sha}; '
                               'nothing to compare (pass --baseline to pick another revision)')
        data = {'metadata': {
            'baseline_sha': baseline_sha,
            'candidate_sha': run(['git', 'rev-parse', 'HEAD'], capture_output=True).stdout.strip(),
            'hostname': platform.node(), 'cpu_count': os.cpu_count(),
            'affinity': sorted(os.sched_getaffinity(0)),
            'lscpu': run(['lscpu'], capture_output=True).stdout,
            'toolchain': run(['rustc', '--version'], capture_output=True).stdout.strip(),
            'scope': 'smoke' if args.smoke_only else 'full',
            'baseline_inline_callbacks': True,
            'control': 'candidate_control reruns the exact candidate executables',
            'quick': args.quick, 'executable_sha256': hashes,
            'started_unix': time.time(),
            'notes': 'No CPU pinning. E2E worker count is per process; suite count is total. '
                     'Split suite partitions client/server workers equally. E2E shards partition '
                     'connections and caller tasks, with equal workers per runtime. Shared handler '
                     'state stays shared. Baseline enables inline-ready dispatch at both ends.'},
            'suite': [], 'e2e': [], 'primitives': [], 'smoke': []}
        def save():
            filename = 'smoke-evaluation.json' if args.smoke_only else 'evaluation.json'
            (output / filename).write_text(json.dumps(data, indent=2) + '\n')
        # Interleave main/candidate and reverse their order on alternating repeats.
        repetitions = 1 if args.quick else 3
        if not args.smoke_only:
            for repeat in range(repetitions):
                for variant in (['main', 'candidate'] if repeat % 2 == 0 else ['candidate', 'main']):
                    result = run([str(binaries[variant]['primitives']), *(['--quick'] if args.quick else [])],
                                 stdout=subprocess.PIPE, timeout=180)
                    rows = json.loads(result.stdout)
                    if not rows:
                        raise RuntimeError('empty component measurements')
                    data['primitives'].append(dict(variant=variant, repeat=repeat, benchmarks=rows))
                    save()
            for workers in [2, 4, 8]:
                for repeat in range(repetitions):
                    # candidate_control reruns the exact candidate executable: its
                    # A/A change measures host and scheduler variation, not an
                    # implementation change. Rotate all three positions so a
                    # consistent run-order bias cannot pass for a real change.
                    variants = ['main', 'candidate', 'candidate_control']
                    variants = variants[repeat % 3:] + variants[:repeat % 3]
                    for split in [False, True]:
                        for variant in variants:
                            binary_variant = 'candidate' if variant == 'candidate_control' else variant
                            label = f'suite-{variant}-w{workers}-split{int(split)}-r{repeat}'
                            destination = output / f'{label}.json'
                            env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers),
                                       RPC_BENCH_SERVER_WORKERS=str(workers // 2 if split else 0),
                                       RPC_BENCH_OUTPUT=str(destination))
                            with (output / f'{label}.log').open('w') as log:
                                run([str(binaries[binary_variant]['suite']), '--json', *(['--quick'] if args.quick else [])],
                                    root=baseline if binary_variant == 'main' else ROOT,
                                    env=env, stdout=log, stderr=log, timeout=300)
                            result = json.loads(destination.read_text())
                            rows = result['scenarios']
                            if len(rows) != 27 or any(r['timed_out'] or r['samples'] == 0 for r in rows):
                                raise RuntimeError(f'failed suite: {label}')
                            data['suite'].append(dict(variant=variant, workers=workers, split=split,
                                                      repeat=repeat, scenarios=rows))
                            save()
                            print(f'{label}: {len(rows)} scenarios passed', flush=True)
                    cases = [(1, 1, 64), (1, 32, 64), (1, 1, 4096), (1, 32, 4096),
                             (4, 32, 64), (4, 32, 4096), (16, 128, 64), (16, 128, 4096)]
                    for case in cases:
                        for shards in [s for s in [1, 2, 4] if s <= workers and s <= case[0]]:
                            for variant in variants:
                                label = f'e2e-{variant}-w{workers}-s{shards}-c{case[0]}-q{case[1]}-b{case[2]}-r{repeat}'
                                binary_variant = 'candidate' if variant == 'candidate_control' else variant
                                with (output / f'{label}.log').open('w') as log:
                                    row = e2e(binaries[binary_variant], workers, shards, case, args.quick, log)
                                data['e2e'].append(dict(variant=variant, workers=workers, shards=shards,
                                                        repeat=repeat, **row))
                                save()
                                print(f'{label}: {row["throughput_rps"]:.0f} RPC/s', flush=True)
        smoke_cases = [('realistic', ['realistic']), ('scaling', ['scaling']),
                       ('throughput', ['throughput']),
                       ('cpu-prefix-ready', ['single', 'cpu-prefix-ready']),
                       ('dynamic-echo', ['single', 'echo', '--connections', '16', '--pool', 'dynamic']),
                       ('cpu-echo', ['single', 'echo', '--pool', 'cpu'])]
        for workers in [2, 8]:
            for repeat in range(repetitions):
                for name, arguments in smoke_cases:
                    variants = ['main', 'candidate', 'candidate_control']
                    for variant in variants[repeat % 3:] + variants[:repeat % 3]:
                        binary_variant = 'candidate' if variant == 'candidate_control' else variant
                        label = f'smoke-{variant}-w{workers}-{name}-r{repeat}'
                        env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers))
                        with (output / f'{label}.log').open('w') as log:
                            result = run([str(binaries[binary_variant]['smoke']), '--skip-build', '--json',
                                          '--duration', '1' if args.quick else '5', *arguments],
                                         env=env, stdout=subprocess.PIPE, stderr=log, timeout=180)
                        rows = json.loads(result.stdout)
                        if not rows or any(r['total_errors'] or r['rpc_counts']['outstanding']
                                           or not r['rpc_counts']['completed'] for r in rows):
                            raise RuntimeError(f'failed smoke workload: {label}')
                        data['smoke'].append(dict(variant=variant, workers=workers, repeat=repeat,
                                                  workload=name, results=rows))
                        save()
                        print(f'{label}: passed', flush=True)
        data['complete'] = True
        save()
    finally:
        run(['git', 'worktree', 'remove', '--force', str(worktree)])


if __name__ == '__main__':
    main()
