#!/usr/bin/env python3
"""Credential-free settings fingerprints and sanitized installed Codex spool checks."""
import argparse
import hashlib
import json
from pathlib import Path


def settings(paths):
    # Explicit settings only: never traverse a home or hash authentication stores.
    allowed = {'config.toml', 'requirements.toml', 'hooks.json', 'settings.json', 'managed_config.toml'}
    if any(p.name not in allowed or p.is_symlink() for p in paths):
        raise ValueError('unsupported settings source')
    return {str(p.resolve()): hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None
            for p in paths}


def fingerprint(path):
    return dict(exists=path.exists(), sha256=hashlib.sha256(path.read_bytes()).hexdigest()
                if path.exists() else None)


def inspect(path, trigger):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    required = {'HOST_SESSION_STARTED', 'HOST_SESSION_ENDING', 'TOOL_EXECUTED', 'CONTEXT_COMPACTED'}
    if not rows or any(r['host'] != 'codex' or r['confidence'] != 'OBSERVED'
                       or r.get('raw_ref') is not None or r['event_type'] not in required
                       or not isinstance(r.get('host_session_id'), str) or not r['host_session_id'] for r in rows):
        raise ValueError('invalid native evidence')
    if not required <= {r['event_type'] for r in rows}:
        raise ValueError('missing native events')
    completed_pair = False
    for session in {r['host_session_id'] for r in rows}:
        events = [r for r in rows if r['host_session_id'] == session]
        kinds = [r['event_type'] for r in events]
        phases = [r['payload'].get('phase') for r in events
                  if r['event_type'] == 'CONTEXT_COMPACTED' and r['payload'].get('trigger') == trigger]
        if (kinds[0] == 'HOST_SESSION_STARTED' and kinds[-1] == 'HOST_SESSION_ENDING'
                and 'TOOL_EXECUTED' in kinds and any(pair == ['PRE', 'POST']
                    for pair in (phases[i:i + 2] for i in range(len(phases) - 1)))):
            completed_pair = True
    if not completed_pair:
        raise ValueError('missing completed same-session compaction pair')
    return dict(events={name: sum(r['event_type'] == name for r in rows) for name in sorted(required)},
                compaction_trigger=trigger, phases=['PRE', 'POST'], **fingerprint(path))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('settings').add_argument('paths', nargs='+', type=Path)
    sub.add_parser('fingerprint').add_argument('path', type=Path)
    positive = sub.add_parser('inspect')
    positive.add_argument('path', type=Path)
    positive.add_argument('--trigger', choices=['manual', 'auto'], required=True)
    compare = sub.add_parser('compare')
    compare.add_argument('before', type=Path)
    compare.add_argument('after', type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'settings':
            result = settings(args.paths)
        elif args.command == 'fingerprint':
            result = fingerprint(args.path)
        elif args.command == 'inspect':
            result = inspect(args.path, args.trigger)
        else:
            if json.loads(args.before.read_text()) != json.loads(args.after.read_text()):
                raise ValueError('settings or inbox changed')
            result = dict(unchanged='PASS')
        print(json.dumps(result, sort_keys=True))
    except Exception:
        raise SystemExit('Host check FAIL; raw event diagnostics suppressed') from None
