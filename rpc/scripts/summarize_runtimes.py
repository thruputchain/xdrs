#!/usr/bin/env python3
"""Summarize raw runtime evaluation reports without discarding repetitions."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
from statistics import median


def control(values, after):
    """Same-binary control column: run-to-run variation, not an implementation change."""
    if not values:
        return ' — | — |'
    return f' {median(values):.0f} ({min(values):.0f}–{max(values):.0f}) | {100*(median(values)/after-1):+.1f}% |'


CONTROL_NOTE = ('The candidate control reruns the exact candidate executable. Its A/A change '
                'measures run-to-run variation, not an implementation change.\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reports', type=Path, nargs='+')
    args = parser.parse_args()
    for report in args.reports:
        data = json.loads(report.read_text())
        print(f'## {report.name}\n')
        print(f"Host: {data['metadata']['hostname']}; commit: {data['metadata']['candidate_sha']}.\n")
        print('Values are medians across process repetitions; ranges are minimum–maximum, '
              'not confidence intervals.\n')
        grouped = defaultdict(lambda: defaultdict(list))
        for run in data.get('primitives', []):
            for row in run['benchmarks']:
                grouped[(row['name'], row['unit'])][run['variant']].append(row.get('median', row.get('mean')))
        if grouped:
            print('| Component | Unit | Main | Candidate | Time change |')
            print('|---|---|---:|---:|---:|')
            for (name, unit), variants in sorted(grouped.items()):
                before, after = median(variants['main']), median(variants['candidate'])
                print(f'| {name} | {unit} | {before:.2f} | {after:.2f} | {100*(after/before-1):+.1f}% |')
            print()
        grouped.clear()
        callback_control = data['metadata'].get('baseline_inline_callbacks', False)
        if not callback_control and any(
                row['name'] == 'bidirectional_rpc' or row['name'].startswith('handler_fanout/')
                for run in data['suite'] for row in run['scenarios']):
            print('Callback comparisons omitted: use the corrected report with inline dispatch at both ends.\n')
        for run in data['suite']:
            for row in run['scenarios']:
                if not callback_control and (row['name'] == 'bidirectional_rpc'
                                             or row['name'].startswith('handler_fanout/')):
                    continue
                if row['params'].get('measurement') == 'run':
                    continue
                key = (run['workers'], run['split'], row['name'])
                grouped[key][run['variant']].append(row['throughput_calls_per_sec'])
        if grouped:
            has_control = any('candidate_control' in variants for variants in grouped.values())
            if has_control:
                print(CONTROL_NOTE)
            print('| Workers | Split client/server | Scenario | Main RPC/s | Candidate RPC/s | Change | Main range | Candidate range |'
                  + (' Control RPC/s (range) | A/A change |' if has_control else ''))
            print('|---:|:---:|---|---:|---:|---:|---:|---:|' + ('---:|---:|' if has_control else ''))
            for (workers, split, name), variants in sorted(grouped.items()):
                before, after = median(variants['main']), median(variants['candidate'])
                values = variants['candidate']
                baseline = variants['main']
                extra = control(variants.get('candidate_control', []), after) if has_control else ''
                print(f'| {workers} | {split} | {name} | {before:.0f} | {after:.0f} | {100*(after/before-1):+.1f}% | {min(baseline):.0f}–{max(baseline):.0f} | {min(values):.0f}–{max(values):.0f} |' + extra)
            print()
        grouped.clear()
        for row in data['e2e']:
            key = (row['workers'], row['shards'], row['connections'], row['concurrency'], row['payload_bytes'])
            grouped[key][row['variant']].append(row)
        if grouped:
            has_control = any('candidate_control' in variants for variants in grouped.values())
            if has_control:
                print(CONTROL_NOTE)
            print('| Workers/process | Runtimes/process | Connections | In flight | Bytes | Main RPC/s | Candidate RPC/s | Change | Candidate p50/p99 µs | Main range | Candidate range |'
                  + (' Control RPC/s (range) | A/A change |' if has_control else ''))
            print('|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|' + ('---:|---:|' if has_control else ''))
            for key, variants in sorted(grouped.items()):
                baseline = [r['throughput_rps'] for r in variants['main']]
                before = median(baseline)
                values = [r['throughput_rps'] for r in variants['candidate']]
                after = median(values)
                p50 = median(r['latency_us']['p50'] for r in variants['candidate'])
                p99 = median(r['latency_us']['p99'] for r in variants['candidate'])
                extra = (control([r['throughput_rps'] for r in variants.get('candidate_control', [])], after)
                         if has_control else '')
                print('| '+' | '.join(map(str, key))+f' | {before:.0f} | {after:.0f} | {100*(after/before-1):+.1f}% | {p50:.1f}/{p99:.1f} | {min(baseline):.0f}–{max(baseline):.0f} | {min(values):.0f}–{max(values):.0f} |' + extra)
            print()
        grouped.clear()
        for run in data.get('smoke', []):
            # Realistic contains several clients of the same profile; retain
            # their aggregate per repetition rather than pretending independent
            # clients are extra benchmark repetitions.
            profiles = defaultdict(float)
            for row in run['results']:
                profiles[(row['profile_name'], row['connections'])] += row['throughput_rps']
            for (name, connections), throughput in profiles.items():
                key = (run['workers'], run['workload'], name, connections)
                grouped[key][run['variant']].append(throughput)
        if grouped:
            print('Smoke connection counts are reported harness values. CpuSetPool discovers its '
                  'actual pool size from LLC topology; its profile count is not the socket count.\n')
            has_control = any('candidate_control' in variants for variants in grouped.values())
            if has_control:
                print(CONTROL_NOTE)
            print('| Workers/process | Workload | Profile | Reported connections | Main RPC/s | Candidate RPC/s | Change | Main range | Candidate range |'
                  + (' Control RPC/s (range) | A/A change |' if has_control else ''))
            print('|---:|---|---|---:|---:|---:|---:|---:|---:|' + ('---:|---:|' if has_control else ''))
            for key, variants in sorted(grouped.items()):
                before, after = median(variants['main']), median(variants['candidate'])
                values = variants['candidate']
                baseline = variants['main']
                extra = control(variants.get('candidate_control', []), after) if has_control else ''
                print('| '+' | '.join(map(str, key))+f' | {before:.0f} | {after:.0f} | {100*(after/before-1):+.1f}% | {min(baseline):.0f}–{max(baseline):.0f} | {min(values):.0f}–{max(values):.0f} |' + extra)
            print()


if __name__ == '__main__':
    main()
