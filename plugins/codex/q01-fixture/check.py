"""Offline fixture checks; no Codex registration or model execution."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parent
market = json.loads((root / '.agents/plugins/marketplace.json').read_text())
entry, = market['plugins']
assert market['name'] == 'receipts-q01-fixture'
assert entry['source'] == {'source': 'local', 'path': './plugins/q01-observer'}
plugin = (root / entry['source']['path']).resolve()
assert plugin.is_relative_to(root)
manifest = json.loads((plugin / '.codex-plugin/plugin.json').read_text())
assert manifest['name'] == entry['name']
assert manifest['skills'] == './skills/' and manifest['hooks'] == './hooks/hooks.json'
assert 'RECEIPTS_Q01_SKILL_DISCOVERED' in (plugin / 'skills/q01-marker/SKILL.md').read_text()
hooks = json.loads((plugin / 'hooks/hooks.json').read_text())['hooks']
assert list(hooks) == ['SessionStart']
handler, = hooks['SessionStart'][0]['hooks']
assert handler == {'type': 'command', 'command': 'python3 "${PLUGIN_ROOT}/hooks/marker.py"', 'timeout': 3}
with tempfile.TemporaryDirectory() as tmp:
    data = Path(tmp) / 'data'
    env = dict(os.environ, PLUGIN_DATA=str(data))
    result = subprocess.run([sys.executable, str(plugin / 'hooks/marker.py')], input=b'SECRET', env=env, capture_output=True, check=True)
    assert result.stdout == result.stderr == b''
    assert (data / 'q01-delivery.log').read_text() == 'SessionStart delivered\n'
    assert sorted(p.name for p in data.iterdir()) == ['q01-delivery.log']
    del env['PLUGIN_DATA']
    assert subprocess.run([sys.executable, str(plugin / 'hooks/marker.py')], env=env, capture_output=True).returncode != 0
print('Q01 fixture offline checks passed')
