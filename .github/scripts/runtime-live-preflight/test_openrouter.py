"""Offline regression controls; all transport and credentials are synthetic."""
import contextlib
import io
import json
from pathlib import Path
import runpy
import sys
import unittest
from unittest.mock import Mock, patch
import urllib.error

probe = runpy.run_path(str(Path(__file__).with_name('openrouter.py')))
MODEL = probe['MODELS'][0]
INVALID = 'receipts/invalid-preflight-model'


def chunk(content='', reason=None, index=0, model=MODEL):
    return dict(model=model, choices=[dict(index=index, finish_reason=reason,
                                          delta=dict(content=content))])


def sse(*chunks, done=True):
    return (''.join('data: ' + json.dumps(c) + '\n\n' for c in chunks)
            + ('data: [DONE]\n\n' if done else '')).encode()


def normal(model=MODEL):
    return json.dumps(dict(model=model, choices=[dict(index=0, finish_reason='stop',
                         message=dict(content='{"answer":"OK"}'))])).encode()


class Validation(unittest.TestCase):
    def test_invalid_model_empty_error(self):
        with self.assertRaises(ValueError):
            probe['validate_error'](400, b'{"error":{}}')

    def test_invalid_model_unrelated_error(self):
        for status in (400, 404):
            with self.subTest(status=status), self.assertRaises(ValueError):
                probe['validate_error'](status, json.dumps(dict(error=dict(
                    code=status, message='Invalid response_format'))).encode())

    def test_wrong_json_model(self):
        with self.assertRaises(ValueError):
            probe['validate'](200, normal('different/model'), False)

    def test_wrong_or_missing_sse_model(self):
        for model in ('different/model', None):
            with self.subTest(model=model), self.assertRaises(ValueError):
                probe['validate'](200, sse(chunk('{"answer":"OK"}', 'stop', model=model)), True)

    def test_mixed_choices(self):
        with self.assertRaises(ValueError):
            probe['validate'](200, sse(chunk('{"answer":', 'stop'),
                                     chunk('"OK"}', index=1)), True)

    def test_valid_model_evidence_and_terminal_sequence(self):
        self.assertEqual(probe['validate'](200, normal(), False), MODEL)
        body = sse(chunk('{"answer":'), chunk('"OK"}'), chunk(reason='stop'))
        self.assertEqual(probe['validate'](200, body, True), MODEL)
        usage = dict(choices=[], usage=dict(total_tokens=10))
        self.assertEqual(probe['validate'](200, sse(chunk('{"answer":"OK"}', 'stop'), usage), True), MODEL)

    def test_additional_identity_choice_and_sequence_refusals(self):
        missing = json.loads(normal()); del missing['model']
        missing_sse = chunk('{"answer":"OK"}', 'stop'); del missing_sse['model']
        multiple = chunk(); multiple['choices'] += chunk(index=1)['choices']
        for raw, stream in [(json.dumps(missing).encode(), False),
                            (sse(missing_sse), True),
                            (sse(multiple), True),
                            (sse(chunk('{"answer":"OK"}', 'stop'), chunk(reason='stop')), True),
                            (sse(chunk('{"answer":'), chunk('"OK"}', 'stop', model='other')), True),
                            (sse(chunk('{"answer":"OK"}', 'stop', index=1)), True),
                            (sse(chunk('{"answer":', 'stop'), chunk('"OK"}')), True),
                            (sse(chunk('{"answer":"OK"}')), True),
                            (sse(chunk('{"answer":"OK"}', 'stop'), done=False), True),
                            (sse(chunk('{"answer":"OK"}', 'stop')) + b'data: {}\n\n', True)]:
            with self.subTest(raw=raw), self.assertRaises((ValueError, KeyError, TypeError)):
                probe['validate'](200, raw, stream)

    def test_usage_trailer_with_finished_choice(self):
        stopped = chunk('{"answer":"OK"}', 'stop')
        trailer = dict(chunk(reason='stop'), usage=dict(total_tokens=10))
        self.assertEqual(probe['validate'](200, sse(stopped, trailer), True), MODEL)
        trailer['choices'][0]['delta'].update(role='assistant', reasoning=None, tool_calls=None)
        self.assertEqual(probe['validate'](200, sse(stopped, trailer), True), MODEL)
        for trailing in (dict(trailer, choices=chunk('extra')['choices']),
                         dict(trailer, choices=chunk(index=1)['choices'])):
            with self.subTest(trailing=trailing), self.assertRaises(ValueError):
                probe['validate'](200, sse(stopped, trailing), True)
        trailer['choices'][0]['delta']['reasoning'] = 'extra'
        with self.assertRaises(ValueError):
            probe['validate'](200, sse(stopped, trailer), True)
        trailer['choices'][0]['delta']['reasoning'] = None
        with self.assertRaises(ValueError):
            probe['validate'](200, sse(stopped, trailer, trailer), True)

    def test_classified_invalid_model(self):
        probe['validate_error'](400, json.dumps(dict(error=dict(
            code=400, message=f'{INVALID} is not a valid model ID'))).encode())
        for error in (dict(code=400, message='another/model is not a valid model ID'),
                      dict(code=400, message=f'Unable to process {INVALID}'),
                      dict(code=404, message=f'{INVALID} is not a valid model ID')):
            with self.subTest(error=error), self.assertRaises(ValueError):
                probe['validate_error'](400, json.dumps(dict(error=error)).encode())


class Harness(unittest.TestCase):
    def test_default_offline_and_optin(self):
        with patch.dict('os.environ', {}, clear=True), patch('urllib.request.build_opener', side_effect=AssertionError('network forbidden')):
            with patch.object(sys, 'argv', ['probe']), contextlib.redirect_stdout(io.StringIO()):
                probe['main']()
            for value in (None, '0'):
                with patch.dict('os.environ', {} if value is None else {'RECEIPTS_OPENROUTER_SMOKE': value}, clear=True), patch.object(sys, 'argv', ['probe', '--live']), self.assertRaises(ValueError):
                    probe['main']()

    def test_bounded_transport(self):
        response = Mock()
        response.__enter__ = Mock(return_value=response)
        response.__exit__ = Mock(return_value=False)
        response.status = 200
        response.read.return_value = b'x' * 65537
        opener = Mock(); opener.open.return_value = response
        with patch('urllib.request.build_opener', return_value=opener) as build:
            status, raw = probe['exchange'](probe['payload'](MODEL, True), 'SYNTHETIC')
        self.assertEqual(build.call_args.args[0].proxies, {})
        self.assertIsNone(build.call_args.args[1].redirect_request(None, None, None, None, None, None))
        request = opener.open.call_args.args[0]
        self.assertEqual(request.full_url, probe['ENDPOINT'])
        self.assertEqual(request.get_header('Authorization'), 'Bearer SYNTHETIC')
        self.assertEqual(opener.open.call_args.kwargs, dict(timeout=20))
        response.read.assert_called_once_with(65537)
        self.assertEqual(status, 200)
        with self.assertRaises(ValueError):
            probe['validate'](status, raw, False)
        error = urllib.error.HTTPError(probe['ENDPOINT'], 429, 'SYNTHETIC', {}, io.BytesIO(b'error'))
        with patch('urllib.request.build_opener', return_value=Mock(open=Mock(side_effect=error))):
            self.assertEqual(probe['exchange']({}, 'SYNTHETIC'), (429, b'error'))

    def test_main_bounds_stop_and_redaction(self):
        source = Path(__file__).with_name('openrouter.py').read_text()
        error = json.dumps(dict(error=dict(code=400, message=f'{INVALID} is not a valid model ID'))).encode()
        for responses, expected_exit, calls in [([(200, normal()), (200, sse(chunk('{"answer":"OK"}', 'stop'))), (400, error)], 0, 3),
                                                 (RuntimeError('SYNTHETIC'), 1, 1),
                                                 ([(200, normal()), (200, sse(chunk('{"answer":"OK"}', 'stop'))), (400, b'{"error":{}}')], 2, 3)]:
            output = io.StringIO()
            namespace = {'__name__': '__main__'}
            # Replace exchange only after its definition, preserving the real CLI handler.
            prefix, suffix = source.split("if __name__ == '__main__':")
            exec(compile(prefix, 'openrouter.py', 'exec'), namespace)
            exchange = Mock(side_effect=responses); namespace['exchange'] = exchange
            with patch.dict('os.environ', {'RECEIPTS_OPENROUTER_SMOKE': '1', 'OPENROUTER_API_KEY': 'SYNTHETIC'}, clear=True), patch.object(sys, 'argv', ['probe', '--live']), patch('signal.signal'), patch('signal.alarm') as alarm, contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                try:
                    exec(compile("if __name__ == '__main__':" + suffix, 'openrouter.py', 'exec'), namespace)
                    code = 0
                except SystemExit as result:
                    code = result.code
            self.assertEqual(code, expected_exit)
            self.assertEqual(exchange.call_count, calls)
            alarm.assert_called_once_with(75)
            self.assertNotIn('SYNTHETIC', output.getvalue())
            if expected_exit == 2:
                self.assertIn('INCONCLUSIVE', output.getvalue())
            if expected_exit == 0:
                self.assertIn('"observed_model": "' + MODEL + '"', output.getvalue())


if __name__ == '__main__':
    unittest.main()
