"""Small repeatable comparison of the same JSON fold contract, not an app benchmark."""
import json
import os
from pathlib import Path
import statistics
import subprocess
import sys
import tempfile
from rust_contract_parity import check_isolation


def main():
    check_isolation()
    binary = Path(os.environ['CARGO_TARGET_DIR']) / 'release/comandos-contract'
    events = []
    for pane in range(8):
        for kind in ['prompt_accepted', 'turn_completed']:
            events.append({'kind': kind, 'evidence': 'confirmed', 'paneKey': f'p{pane}',
                           'turnId': f't{pane}', 'processKey': f'pid{pane}', 'eventId': f'{pane}-{kind}',
                           'occurredAtMs': 123456, 'correlation': 'source'})
    request = json.dumps({'op': 'fold', 'events': events}) + '\n'
    oracle = ('import json,sys;sys.path.insert(0,"/work/lib");from turn_state import turns_from_events\n'
              'for line in sys.stdin:\n print(json.dumps(turns_from_events(json.loads(line)["events"]),ensure_ascii=False))\n')
    commands = {'python': [sys.executable, '-c', oracle], 'rust': [str(binary)]}
    measurements = {name: [] for name in commands}
    outputs = {}
    with tempfile.TemporaryDirectory() as directory:
        input_path = Path(directory) / 'input'
        input_path.write_text(request * 2000)
        for _ in range(5):
            for name, command in commands.items():
                output_path = Path(directory) / name
                timing_path = Path(directory) / 'timing'
                with input_path.open('rb') as input_file, output_path.open('wb') as output_file:
                    # GNU time forks from its small native process, avoiding
                    # Python's inherited pre-exec high-water RSS in wait4.
                    subprocess.run(['/usr/bin/time', '-f', '%e %U %S %M', '-o', str(timing_path), *command],
                                   stdin=input_file, stdout=output_file, check=True, timeout=30)
                    wall, user, system, rss = timing_path.read_text().split()
                    measurements[name].append({'wallSeconds': float(wall),
                                               'cpuSeconds': float(user) + float(system),
                                               'maxRssKiB': int(rss)})
                outputs[name] = output_path
        with outputs['python'].open() as a, outputs['rust'].open() as b:
            assert [json.loads(line) for line in a] == [json.loads(line) for line in b]
    result = {'scope': '2000 JSONL folds of 16 events, 5 fresh processes per implementation',
              'binaryBytes': binary.stat().st_size,
              'median': {name: {field: statistics.median(row[field] for row in rows) for field in rows[0]}
                         for name, rows in measurements.items()}, 'runs': measurements,
              'limitation': 'Synthetic pure-core workload; not whole-app memory, CPU or UI latency.'}
    output = Path('/work/.migration-build/core-benchmark.json')
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: value for key, value in result.items() if key != 'runs'}, indent=2))


if __name__ == '__main__':
    main()
