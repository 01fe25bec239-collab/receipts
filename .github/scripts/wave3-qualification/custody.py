#!/usr/bin/env python3
"""Synthetic-only Linux UID custody controls. No provider CLI, login or network."""
import json
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tempfile


def main():
    if sys.platform != 'linux' or os.geteuid() != 0:
        raise ValueError('disposable Linux root required')
    uids = (63171, 63172)
    for uid in uids:
        try:
            pwd.getpwuid(uid)
        except KeyError:
            continue
        raise ValueError('fixture UID already assigned')
    if any(p.stat().st_uid in uids for p in Path('/proc').iterdir() if p.name.isdigit()):
        raise ValueError('fixture UID has live processes')
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='receipts-synthetic-custody-') as tmp:
        root = Path(tmp)
        root.chmod(0o711)
        home = root / 'home'
        home.mkdir(mode=0o700)
        os.chown(home, uids[0], uids[0])
        secret = home / 'sentinel'
        secret.write_text('SYNTHETIC_ONLY_NOT_A_PROVIDER_CREDENTIAL')
        secret.chmod(0o600)
        os.chown(secret, uids[0], uids[0])
        def child(uid, code):
            def identity():
                os.setgroups([])
                os.setgid(uid)
                os.setuid(uid)
            return subprocess.run([sys.executable, '-I', '-c', code, str(secret)],
                env={'HOME': str(home), 'CODEX_HOME': str(home), 'CLAUDE_CONFIG_DIR': str(home),
                     'PATH': '/usr/bin:/bin'}, cwd='/', preexec_fn=identity,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
        read = 'import pathlib,sys; p=pathlib.Path(sys.argv[1]); assert p.read_text().startswith("SYNTHETIC_ONLY")'
        assert child(uids[0], read).returncode == 0
        denied = 'import pathlib,sys\ntry: pathlib.Path(sys.argv[1]).read_text()\nexcept PermissionError: pass\nelse: raise AssertionError("cross UID read allowed")'
        assert child(uids[1], denied).returncode == 0
        refresh = 'import pathlib,sys,os; assert not any(k.endswith("API_KEY") for k in os.environ); pathlib.Path(sys.argv[1]).write_text("SYNTHETIC_ONLY_REFRESH")'
        assert child(uids[0], refresh).returncode == 0
        secret.unlink()
        missing = 'import pathlib,sys\ntry: pathlib.Path(sys.argv[1]).read_text()\nexcept FileNotFoundError: pass\nelse: raise AssertionError("revoked store remained")'
        assert child(uids[0], missing).returncode == 0
        assert child(uids[0], 'import pathlib,sys; assert not pathlib.Path(sys.argv[1]).exists()').returncode == 0
    assert not root.exists()
    print(json.dumps(dict(synthetic_custody='PASS', owner_read='PASS', cross_uid_denial='PASS',
                         refresh='PASS', missing_store='PASS', revocation='PASS', cleanup='PASS',
                         provider_execution=False)))


if __name__ == '__main__':
    try:
        main()
    except Exception:
        raise SystemExit('synthetic custody FAIL; stopped') from None
