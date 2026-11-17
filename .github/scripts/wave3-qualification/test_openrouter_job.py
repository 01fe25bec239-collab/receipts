import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import shutil
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
WRAPPER = Path('.github/scripts/wave3-qualification/openrouter_job.py')
PROBE = Path('.github/scripts/runtime-live-preflight/openrouter.py')
job = runpy.run_path(str(ROOT / WRAPPER))


class JobControls(unittest.TestCase):
    def setUp(self):
        self.workspace = patch.dict(os.environ, {'GITHUB_WORKSPACE': str(ROOT)})
        self.workspace.start()
        self.addCleanup(self.workspace.stop)

    def test_optin_and_missing_secret(self):
        for env in ({'GITHUB_WORKSPACE': str(ROOT)},
                    {'GITHUB_WORKSPACE': str(ROOT), 'RECEIPTS_OPENROUTER_SMOKE': '1'}):
            with patch.dict(os.environ, env, clear=True), self.assertRaises(ValueError):
                job['execute'](True)

    def test_child_isolation_redaction_and_bounds(self):
        out = io.StringIO()
        env = dict(GITHUB_WORKSPACE=str(ROOT), RECEIPTS_OPENROUTER_SMOKE='1', OPENROUTER_API_KEY='SYNTHETIC_ONLY',
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
            self.assertEqual(run.call_args.args[0][1:], ['-I', '-', '--live'])
            self.assertEqual(child['input'], (ROOT / PROBE).read_bytes())
            self.assertEqual(hashlib.sha256(child['input']).hexdigest(), job['PROBE_SHA256'])
            self.assertFalse(Path(child['env']['HOME']).exists())
            for returncode, stdout, stderr in [(1, b'SYNTHETIC_ONLY', b''),
                                               (0, b'SYNTHETIC_ONLY', b''),
                                               (2, b'', b'SYNTHETIC_ONLY')]:
                run.return_value = subprocess.CompletedProcess([], returncode, stdout, stderr)
                with self.assertRaises(ValueError), contextlib.redirect_stdout(out):
                    job['execute'](True)
        self.assertNotIn('SYNTHETIC_ONLY', out.getvalue())


@contextlib.contextmanager
def checkout_fixture():
    with tempfile.TemporaryDirectory(prefix='receipts-probe-path-') as tmp:
        root = Path(tmp).resolve() / 'checkout'
        for relative in (WRAPPER, PROBE):
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, target)
        yield root


class LaunchBinding(unittest.TestCase):
    def test_ancestor_and_leaf_redirects_refuse_before_launch(self):
        for relative in (Path('.github'), Path('.github/scripts'), PROBE.parent,
                         WRAPPER.parent, PROBE, WRAPPER):
            with self.subTest(path=str(relative)), checkout_fixture() as root:
                module = runpy.run_path(str(root / WRAPPER))
                path = root / relative
                moved = root.parent / 'redirected'
                path.rename(moved)
                path.symlink_to(moved, target_is_directory=moved.is_dir())
                with patch.dict(os.environ, {'GITHUB_WORKSPACE': str(root)}, clear=True), \
                        patch('subprocess.run') as child, self.assertRaises(ValueError):
                    module['execute']()
                child.assert_not_called()

    def test_missing_or_redirected_checkout_refuses(self):
        with checkout_fixture() as root:
            module = runpy.run_path(str(root / WRAPPER))
            alias = root.parent / 'alias'
            alias.symlink_to(root, target_is_directory=True)
            for env in ({}, {'GITHUB_WORKSPACE': str(alias)}, {'GITHUB_WORKSPACE': '.'}):
                with self.subTest(env=env), patch.dict(os.environ, env, clear=True), \
                        patch('subprocess.run') as child, self.assertRaises(ValueError):
                    module['execute']()
                child.assert_not_called()

    def test_mismatched_launcher_checkout_and_probe_content_refuse(self):
        with checkout_fixture() as root, checkout_fixture() as other:
            module = runpy.run_path(str(other / WRAPPER))
            with patch.dict(os.environ, {'GITHUB_WORKSPACE': str(root)}, clear=True), \
                    patch('subprocess.run') as child, self.assertRaises(ValueError):
                module['execute']()
            child.assert_not_called()
            module = runpy.run_path(str(root / WRAPPER))
            (root / PROBE).write_text('raise AssertionError("UNREVIEWED_TARGET")')
            with patch.dict(os.environ, {'GITHUB_WORKSPACE': str(root)}, clear=True), \
                    patch('subprocess.run') as child, self.assertRaises(ValueError):
                module['execute']()
            child.assert_not_called()

    def test_launch_consumes_the_validated_bytes_even_after_path_replacement(self):
        with checkout_fixture() as root:
            module = runpy.run_path(str(root / WRAPPER))
            reviewed = (root / PROBE).read_bytes()
            def launch(argv, **kwargs):
                (root / PROBE).write_text('raise AssertionError("DIFFERENT_LAUNCH_TARGET")')
                self.assertEqual(argv[1:], ['-I', '-'])
                self.assertEqual(kwargs['input'], reviewed)
                self.assertEqual(hashlib.sha256(kwargs['input']).hexdigest(), module['PROBE_SHA256'])
                return subprocess.CompletedProcess([], 0, b'', b'')
            with patch.dict(os.environ, {'GITHUB_WORKSPACE': str(root)}, clear=True), \
                    patch('subprocess.run', side_effect=launch), contextlib.redirect_stdout(io.StringIO()):
                module['execute']()


if __name__ == '__main__':
    unittest.main()
