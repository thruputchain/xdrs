#!/usr/bin/env python3
"""Store complete RPC measurements in the results repository."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--branch', required=True, help='source branch; selects the series directory')
    parser.add_argument('--commit', required=True)
    parser.add_argument('--commit-time', type=int, required=True)
    parser.add_argument('--history-url', required=True, help='git URL of the results repository')
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{40}', args.commit):
        parser.error('--commit must be a full commit SHA')
    metadata = json.loads((args.report / 'metadata.json').read_text())
    e2e = json.loads((args.report / 'e2e.json').read_text())
    primitives = json.loads((args.report / 'primitives.json').read_text())
    if metadata['git_sha'] != args.commit or e2e['metadata'] != metadata or metadata['quick']:
        raise ValueError('only full measurements of the requested commit may be published')
    expected = {(c, q, b, pos, inline) for c, q, b in
                [(1, 1, 64), (1, 32, 64), (1, 1, 4096), (1, 32, 4096), (4, 32, 64)]
                for pos in range(1, 5) for inline in [True]}
    actual = {(r['connections'], r['concurrency'], r['payload_bytes'],
               r['run_position'], r['server_inline_ready_dispatch']) for r in e2e['runs']}
    if actual != expected or len(e2e['runs']) != len(expected) or not primitives:
        raise ValueError('incomplete benchmark matrix')
    for row in e2e['runs']:
        if row['errors'] or row['completed'] != row['concurrency'] * row['iterations_per_worker']:
            raise ValueError('failed or incomplete RPC measurement')
    timestamp = datetime.fromtimestamp(args.commit_time, timezone.utc).strftime('%Y-%m-%dT%H:%M:%S')
    filename = f'{timestamp}_{args.commit}'
    files = {
        f'metrics/xdr-rpc-e2e/{args.branch}/{filename}': e2e,
        f'metrics/xdr-rpc-components/{args.branch}/{filename}': {'metadata': metadata, 'benchmarks': primitives},
    }
    evaluation_path = args.report / 'evaluation/evaluation.json'
    if evaluation_path.exists():
        evaluation = json.loads(evaluation_path.read_text())
        if (not evaluation.get('complete') or evaluation['metadata']['quick']
                or evaluation['metadata']['candidate_sha'] != args.commit):
            raise ValueError('runtime evaluation is incomplete or has the wrong source commit')
        files[f'metrics/xdr-rpc-runtime-evaluation/{args.branch}/{filename}'] = evaluation
    decode_path = args.report / 'reply-decode/evaluation.json'
    if decode_path.exists():
        decoded = json.loads(decode_path.read_text())
        if (not decoded.get('complete') or decoded['metadata']['quick']
                or decoded['metadata']['prebuilt'] or len(decoded['runs']) != 216
                or decoded['metadata']['candidate_sha'] != args.commit):
            raise ValueError('reply decode evaluation is incomplete or has the wrong source commit')
        files[f'metrics/xdr-rpc-reply-decode-evaluation/{args.branch}/{filename}'] = decoded
    env = dict(os.environ, GIT_TERMINAL_PROMPT='0')
    with tempfile.TemporaryDirectory(prefix='rpc-metrics-history-') as directory:
        def git(*argv, check=True):
            return subprocess.run(['git', '-C', directory, *argv], check=check, env=env)
        git('init', '--quiet')
        git('remote', 'add', 'origin', args.history_url)
        git('config', 'user.name', 'metrics-script')
        git('config', 'user.email', 'metrics-script@users.noreply.github.com')
        # Populate the index, not the historical working tree. On a
        # concurrent push, refresh the parent and stage only this run's files.
        for attempt in range(5):
            git('fetch', '--depth=1', '--filter=blob:none', '--quiet', 'origin', 'main')
            git('update-ref', 'refs/heads/main', 'FETCH_HEAD')
            git('symbolic-ref', 'HEAD', 'refs/heads/main')
            git('read-tree', 'HEAD')
            for path, content in files.items():
                destination = Path(directory) / path
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_text(json.dumps(content, indent=2) + '\n')
                git('add', '--', path)
            if git('diff', '--cached', '--quiet', check=False).returncode == 0:
                print('Measurements are already recorded.')
                return
            git('commit', '--quiet', '-m', f'Add metrics for xdr_rpc {args.branch} {args.commit}')
            if git('push', '--quiet', 'origin', 'HEAD:main', check=False).returncode == 0:
                print(f'Recorded RPC metrics for {args.commit}')
                return
            print(f'Metrics push failed; refreshing parent (attempt {attempt + 1}/5).')
        raise RuntimeError('could not publish metrics after five attempts')


if __name__ == '__main__':
    main()
