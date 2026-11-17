import json
from pathlib import Path
import runpy
import tempfile
import unittest

host = runpy.run_path(str(Path(__file__).with_name('host.py')))


class HostControls(unittest.TestCase):
    def test_positive_and_refusals(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            settings = root / 'config.toml'
            before = host['settings']([settings])
            settings.write_text('features.hooks = false\n')
            self.assertNotEqual(before, host['settings']([settings]))
            with self.assertRaises(ValueError):
                host['settings']([root / 'auth.json'])
            spool = root / 'spool'
            rows = [dict(host='codex', confidence='OBSERVED', host_session_id='synthetic-session', event_type=kind, payload=payload)
                    for kind, payload in [('HOST_SESSION_STARTED', {}), ('TOOL_EXECUTED', {}),
                                          ('CONTEXT_COMPACTED', dict(phase='PRE', trigger='auto')),
                                          ('CONTEXT_COMPACTED', dict(phase='POST', trigger='auto')),
                                          ('HOST_SESSION_ENDING', {})]]
            def save():
                spool.write_text('\n'.join(json.dumps(r) for r in rows))
            save()
            self.assertEqual(host['inspect'](spool, 'auto')['compaction_trigger'], 'auto')
            with self.assertRaises(ValueError):
                host['inspect'](spool, 'manual')
            rows[3]['host_session_id'] = 'other'; save()
            with self.assertRaises(ValueError):
                host['inspect'](spool, 'auto')
            rows[3]['host_session_id'] = 'synthetic-session'
            rows[2], rows[3] = rows[3], rows[2]; save()
            with self.assertRaises(ValueError):
                host['inspect'](spool, 'auto')
            rows[2], rows[3] = rows[3], rows[2]
            rows[1]['confidence'] = 'INFERRED'; save()
            with self.assertRaises(ValueError):
                host['inspect'](spool, 'auto')
            rows[1]['confidence'] = 'OBSERVED'; rows.pop(3); save()
            with self.assertRaises(ValueError):
                host['inspect'](spool, 'auto')


if __name__ == '__main__':
    unittest.main()
