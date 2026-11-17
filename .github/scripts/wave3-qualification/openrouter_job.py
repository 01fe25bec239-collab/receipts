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

PROBE_SHA256 = '4558895a08e4aa6a7433c3c72f38597a177ebc1339921acda3b19cfd08088295'


def reviewed_probe():
    checkout = Path(os.environ.get('GITHUB_WORKSPACE', ''))
    if not checkout.is_absolute() or checkout.resolve(strict=True) != checkout:
        raise ValueError('canonical checkout required')
    wrapper = checkout / '.github/scripts/wave3-qualification/openrouter_job.py'
    probe = checkout / '.github/scripts/runtime-live-preflight/openrouter.py'
    if Path(__file__).absolute() != wrapper:
        raise ValueError('launcher outside checkout')
    for path in (wrapper, probe):
        if path.resolve(strict=True) != path or not path.is_file():
            raise ValueError('redirected executable source')
    source = probe.read_bytes()
    if hashlib.sha256(source).hexdigest() != PROBE_SHA256:
        raise ValueError('unreviewed probe')
    return source


def execute(live=False):
    source = reviewed_probe()
    with tempfile.TemporaryDirectory(prefix='receipts-openrouter-') as home:
        env = {'PATH': '/usr/bin:/bin', 'HOME': home, 'LANG': 'C.UTF-8'}
        if live:
            if os.environ.get('RECEIPTS_OPENROUTER_SMOKE') != '1':
                raise ValueError('explicit opt-in required')
            key = os.environ.get('OPENROUTER_API_KEY')
            if not key or '\n' in key or '\r' in key:
                raise ValueError('eligible operator secret required')
            env.update(OPENROUTER_API_KEY=key, RECEIPTS_OPENROUTER_SMOKE='1')
        # Launch these validated bytes, so a later pathname change cannot select other code.
        result = subprocess.run([sys.executable, '-I', '-'] + (['--live'] if live else []),
                                input=source, env=env, cwd=home, stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, timeout=85)
        # Emit only validated success records. Unexpected output is never echoed.
        expected = [json.dumps(dict(model='openai/gpt-4.1-mini', observed_model='openai/gpt-4.1-mini',
                                    stream=stream, verdict='PASS')) for stream in (False, True)]
        expected.append('invalid-model error PASS')
        if result.returncode or (live and result.stdout.decode().splitlines() != expected) or result.stderr:
            raise ValueError('probe did not qualify')
        print(json.dumps(dict(mode='live-http' if live else 'offline', verdict='PASS',
                              probe_sha256=hashlib.sha256(source).hexdigest(),
                              requests=3 if live else 0)))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--live', action='store_true')
    args = parser.parse_args()
    try:
        execute(args.live)
    except Exception:
        raise SystemExit('OpenRouter job FAIL or INCONCLUSIVE; stopped without retry') from None
