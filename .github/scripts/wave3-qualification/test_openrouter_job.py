import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import unittest
from unittest.mock import patch

job = runpy.run_path(str(Path(__file__).with_name('openrouter_job.py')))


class JobControls(unittest.TestCase):
    def test_optin_and_missing_secret(self):
        for env in ({}, {'RECEIPTS_OPENROUTER_SMOKE': '1'}):
            with patch.dict(os.environ, env, clear=True), self.assertRaises(ValueError):
                job['execute'](True)

    def test_child_isolation_redaction_and_bounds(self):
        out = io.StringIO()
        env = dict(RECEIPTS_OPENROUTER_SMOKE='1', OPENROUTER_API_KEY='SYNTHETIC_ONLY',
                   ANTHROPIC_API_KEY='DO_NOT_FORWARD', HTTP_PROXY='DO_NOT_FORWARD',
                   CODEX_HOME='/do/not/use', CLAUDE_CONFIG_DIR='/do/not/use')
        records = [json.dumps(dict(model='openai/gpt-4.1-mini', observed_model='openai/gpt-4.1-mini',
                                   stream=s, verdict='PASS')) for s in (False, True)]
        records.append('invalid-model error PASS')
        with patch.dict(os.environ, env, clear=True), patch('subprocess.run') as run:
            run.return_value = subprocess.CompletedProcess([], 0, '\n'.join(records).encode(), b'')
            with contextlib.redirect_stdout(out):
                job['execute'](True)
            child = run.call_args.kwargs
            self.assertEqual(set(child['env']), {'HOME', 'PATH', 'LANG', 'OPENROUTER_API_KEY', 'RECEIPTS_OPENROUTER_SMOKE'})
            self.assertEqual(child['timeout'], 85)
            self.assertFalse(Path(child['env']['HOME']).exists())
            for returncode, stdout, stderr in [(1, b'SYNTHETIC_ONLY', b''),
                                               (0, b'SYNTHETIC_ONLY', b''),
                                               (2, b'', b'SYNTHETIC_ONLY')]:
                run.return_value = subprocess.CompletedProcess([], returncode, stdout, stderr)
                with self.assertRaises(ValueError), contextlib.redirect_stdout(out):
                    job['execute'](True)
        self.assertNotIn('SYNTHETIC_ONLY', out.getvalue())


if __name__ == '__main__':
    unittest.main()
