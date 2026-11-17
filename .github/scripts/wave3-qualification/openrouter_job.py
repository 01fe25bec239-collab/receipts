#!/usr/bin/env python3
"""Launch the unchanged bounded probe with an allowlisted child environment."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

PROBE = Path(__file__).resolve().parents[1] / 'runtime-live-preflight/openrouter.py'


def execute(live=False):
    with tempfile.TemporaryDirectory(prefix='receipts-openrouter-') as home:
        env = {'PATH': '/usr/bin:/bin', 'HOME': home, 'LANG': 'C.UTF-8'}
        if live:
            if os.environ.get('RECEIPTS_OPENROUTER_SMOKE') != '1':
                raise ValueError('explicit opt-in required')
            key = os.environ.get('OPENROUTER_API_KEY')
            if not key or '\n' in key or '\r' in key:
                raise ValueError('eligible operator secret required')
            env.update(OPENROUTER_API_KEY=key, RECEIPTS_OPENROUTER_SMOKE='1')
        result = subprocess.run([sys.executable, '-I', str(PROBE)] + (['--live'] if live else []),
                                env=env, cwd=home, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=85)
        # Emit only validated success records. Unexpected output is never echoed.
        expected = [json.dumps(dict(model='openai/gpt-4.1-mini', observed_model='openai/gpt-4.1-mini',
                                    stream=stream, verdict='PASS')) for stream in (False, True)]
        expected.append('invalid-model error PASS')
        if result.returncode or (live and result.stdout.decode().splitlines() != expected) or result.stderr:
            raise ValueError('probe did not qualify')
        print(json.dumps(dict(mode='live-http' if live else 'offline', verdict='PASS',
                              probe_sha256=hashlib.sha256(PROBE.read_bytes()).hexdigest(),
                              requests=3 if live else 0)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--live', action='store_true')
    args = parser.parse_args()
    try:
        execute(args.live)
    except Exception:
        raise SystemExit('OpenRouter job FAIL or INCONCLUSIVE; stopped without retry') from None
