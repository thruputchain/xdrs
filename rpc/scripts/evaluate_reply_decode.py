#!/usr/bin/env python3
"""Compare reader-side reply decoding with main and the retained owned API."""
import argparse
import json
import os
from pathlib import Path
import platform
import selectors
import subprocess
import time

from evaluate_runtimes import ROOT, digest, resolve_baseline, run

# workers/process, connections, inflight, payload, measured calls/caller.
CASES = [(2, 1, 32, 64, 100_000), (8, 1, 32, 4096, 30_000),
         (8, 1, 32, 65536, 5000)]
LABELS = ['main', 'decoded', 'owned', 'main_control', 'decoded_control', 'owned_control']


def fixed_cpus(workers):
    cores = {}
    for cpu in sorted(os.sched_getaffinity(0)):
        path = Path(f'/sys/devices/system/cpu/cpu{cpu}/topology')
        core = tuple((path / name).read_text().strip() for name in ['physical_package_id', 'core_id'])
        cores.setdefault(core, cpu)
    if len(cores) < 2 * workers:
        raise RuntimeError('not enough physical cores; use --unpinned-only for local diagnostics')
    cpus = list(cores.values())[:2 * workers]
    return {'server': cpus[:workers], 'client': cpus[workers:]}


def measure(bins, label, case, mask, quick, log):
    workers, connections, inflight, payload, iterations = case
    iterations, warmup = (100, 20) if quick else (iterations, 5000)
    variant = label.removesuffix('_control')
    env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers))
    def pin(role):
        return ['taskset', '-c', ','.join(map(str, mask[role]))] if mask else []
    server = subprocess.Popen(pin('server') + [str(bins['main'] / 'rpc-smoke-server'),
                              '--port', '0', '--runtime-shards', '1'],
                              env=env, stdout=subprocess.PIPE, stderr=log, text=True)
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(server.stdout, selectors.EVENT_READ)
            if not selector.select(timeout=15):
                raise RuntimeError('server readiness timed out')
            ready = server.stdout.readline().strip()
        if not ready.startswith('READY ') or ',' in ready:
            raise RuntimeError(f'invalid readiness: {ready!r}')
        client = bins['main' if variant == 'main' else 'candidate'] / 'rpc-bench-client'
        result = run(pin('client') + [str(client), '--addr', ready[6:], '--runtime-shards', '1',
                     '--connections', str(connections), '--concurrency', str(inflight),
                     '--payload-bytes', str(payload), '--iterations', str(iterations),
                     '--warmup', str(warmup), *(['--decode-replies'] if variant == 'decoded' else [])],
                     env=env, stdout=subprocess.PIPE, stderr=log, timeout=180)
        row = json.loads(result.stdout)
        if row['errors'] or row['completed'] != inflight * iterations or row['decode_replies'] != (variant == 'decoded'):
            raise RuntimeError(f'incomplete/wrong-mode measurement: {row}')
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
    parser.add_argument('--unpinned-only', action='store_true')
    parser.add_argument('--repetitions', type=int, default=6)
    parser.add_argument('--prebuilt-main', type=Path)
    parser.add_argument('--prebuilt-candidate', type=Path)
    parser.add_argument('--baseline', help='revision to compare against (default: merge-base with origin/main)')
    args = parser.parse_args()
    if bool(args.prebuilt_main) != bool(args.prebuilt_candidate) or args.repetitions < 1:
        parser.error('provide both prebuilt directories and a positive repetition count')
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    worktree = None
    baseline_sha = None
    try:
        if args.prebuilt_main:
            bins = {'main': args.prebuilt_main.resolve(), 'candidate': args.prebuilt_candidate.resolve()}
        else:
            baseline_sha = resolve_baseline(args.baseline)
            worktree = output / 'main-source'
            run(['git', 'worktree', 'add', '--detach', str(worktree), baseline_sha])
            # The worktree holds the whole repository; the RPC workspace
            # members sit under the same subdirectory as this script's ROOT.
            toplevel = run(['git', 'rev-parse', '--show-toplevel'], capture_output=True).stdout.strip()
            baseline = worktree / ROOT.relative_to(Path(toplevel).resolve())
            # The exact same benchmark runs in both builds. Only candidate
            # enables the benchmark feature that can invoke the new API.
            for name in ['rpc-smoke/src/bench_client.rs', 'rpc-smoke/Cargo.toml']:
                (baseline / name).write_bytes((ROOT / name).read_bytes())
            bins = {}
            with (output / 'build.log').open('w') as log:
                for variant, source in [('main', baseline), ('candidate', ROOT)]:
                    target = output / f'{variant}-target'
                    run(['cargo', 'build', '--locked', '--release', '-p', 'rpc-smoke',
                         *(['--features', 'reply-decode'] if variant == 'candidate' else [])],
                        root=source, env=dict(os.environ, CARGO_TARGET_DIR=str(target)),
                        stdout=log, stderr=log, timeout=900)
                    bins[variant] = target / 'release'
        masks = {} if args.unpinned_only else {w: fixed_cpus(w) for w, *_ in CASES}
        data = {'metadata': {
            'baseline_sha': baseline_sha,
            'candidate_sha': run(['git', 'rev-parse', 'HEAD'], capture_output=True).stdout.strip(),
            'hostname': platform.node(), 'lscpu': run(['lscpu'], capture_output=True).stdout,
            'toolchain': run(['rustc', '--version'], capture_output=True).stdout.strip(),
            'affinity': sorted(os.sched_getaffinity(0)), 'fixed_affinity': masks,
            'quick': args.quick, 'prebuilt': bool(args.prebuilt_main),
            'repetitions': 1 if args.quick else args.repetitions, 'cases': CASES,
            'started_unix': time.time(),
            'executable_sha256': {v: {name: digest(path / name) for name in ['rpc-bench-client', 'rpc-smoke-server']}
                                  for v, path in bins.items()},
            'notes': 'Main server in every row. Main/owned/decoded client paths plus exact-binary '
                     'controls. Owned and decoded use the same candidate binary with a runtime flag. '
                     'Both modes validate the same payload. One runtime/process. No allocation instrumentation.'},
            'runs': [], 'complete': False}
        path = output / 'evaluation.json'
        for repeat in range(data['metadata']['repetitions']):
            offset = repeat % len(LABELS)
            labels = LABELS[offset:] + LABELS[:offset]
            for case in CASES:
                placements = [False] if args.unpinned_only else ([False, True] if repeat % 2 == 0 else [True, False])
                for pinned in placements:
                    for label in labels:
                        name = f'{label}-w{case[0]}-b{case[3]}-pin{int(pinned)}-r{repeat}'
                        with (output / f'{name}.log').open('w') as log:
                            row = measure(bins, label, case, masks[case[0]] if pinned else None, args.quick, log)
                        data['runs'].append(dict(variant=label, workers=case[0], pinned=pinned, repeat=repeat, **row))
                        path.write_text(json.dumps(data, indent=2) + '\n')
                        print(f'{name}: {row["throughput_rps"]:.0f} RPC/s over {row["wall_seconds"]:.2f}s', flush=True)
        expected = data['metadata']['repetitions'] * len(CASES) * len(LABELS) * (1 if args.unpinned_only else 2)
        if len(data['runs']) != expected:
            raise RuntimeError('incomplete comparison')
        data['complete'] = True
        path.write_text(json.dumps(data, indent=2) + '\n')
    finally:
        if worktree is not None:
            run(['git', 'worktree', 'remove', '--force', str(worktree)])


if __name__ == '__main__':
    main()
