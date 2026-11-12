#!/usr/bin/env python3
"""Bounded direct OpenRouter probe. Default execution is entirely offline."""
import argparse
import json
import os
import signal
import sys
import urllib.error
import urllib.request

ENDPOINT = 'https://openrouter.ai/api/v1/chat/completions'
MODELS = ('openai/gpt-4.1-mini',)
LIMIT = 65536


def payload(model, stream):
    return dict(model=model, messages=[dict(role='user', content='Return only {"answer":"OK"}.')],
                max_tokens=64, temperature=0, stream=stream,
                provider=dict(require_parameters=True, allow_fallbacks=False),
                response_format=dict(type='json_schema', json_schema=dict(name='smoke', strict=True,
                    schema=dict(type='object', properties=dict(answer=dict(type='string', enum=['OK'])),
                                required=['answer'], additionalProperties=False))))


def validate(status, raw, stream):
    if status != 200:
        raise ValueError('http-error')
    if len(raw) > LIMIT:
        raise ValueError('capture-limit')
    if stream:
        content, done, stopped = '', False, False
        for event in raw.decode().replace('\r\n', '\n').split('\n\n'):
            lines = [line[5:].lstrip() for line in event.splitlines() if line.startswith('data:')]
            if not lines:
                continue
            data = '\n'.join(lines)
            if done:
                raise ValueError('event-after-done')
            if data == '[DONE]':
                done = True
                continue
            item = json.loads(data)
            if 'error' in item:
                raise ValueError('stream-error')
            for choice in item.get('choices', []):
                reason = choice.get('finish_reason')
                if reason is not None and reason != 'stop':
                    raise ValueError('incomplete-stream')
                stopped |= reason == 'stop'
                content += choice.get('delta', {}).get('content') or ''
        if not done or not stopped:
            raise ValueError('incomplete-stream')
    else:
        item = json.loads(raw)
        if 'error' in item or item['choices'][0]['finish_reason'] != 'stop':
            raise ValueError('completion-error')
        content = item['choices'][0]['message']['content']
    if json.loads(content) != {'answer': 'OK'}:
        raise ValueError('schema-mismatch')


def exchange(body, key):
    # No ambient proxy or redirect may receive the Authorization header.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    req = urllib.request.Request(ENDPOINT, json.dumps(body).encode(),
                                 {'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'})
    try:
        with opener.open(req, timeout=20) as response:
            return response.status, response.read(LIMIT + 1)
    except urllib.error.HTTPError as error:
        with error:
            return error.code, error.read(LIMIT + 1)


def validate_error(status, raw):
    if status not in (400, 404) or len(raw) > LIMIT or not isinstance(json.loads(raw).get('error'), dict):
        raise ValueError('expected invalid-model error')


def offline():
    normal = json.dumps(dict(choices=[dict(finish_reason='stop', message=dict(content='{"answer":"OK"}'))])).encode()
    chunk = json.dumps(dict(choices=[dict(finish_reason='stop', delta=dict(content='{"answer":"OK"}'))]))
    sse = (': keepalive\n\ndata: ' + chunk + '\n\ndata: [DONE]\n\n').encode()
    validate(200, normal, False)
    validate(200, sse, True)
    cases = [(401, normal, False), (429, normal, False), (500, normal, False),
             (200, b'data: {"error":{"code":429}}\n\n', True),
             (200, sse.replace(b'[DONE]', b'{}'), True), (200, b'', True),
             (200, normal.replace(b'OK', b'NO'), False), (200, b'x' * (LIMIT + 1), False),
             (200, sse.replace(b'"stop"', b'"length"'), True)]
    for args in cases:
        try:
            validate(*args)
        except (ValueError, KeyError, TypeError):
            continue
        raise AssertionError('accepted invalid response')
    validate_error(400, b'{"error":{"code":400}}')
    for status, raw in [(200, b'{"error":{}}'), (401, b'{"error":{}}'),
                        (400, b'{}'), (404, b'{"error":"bad"}')]:
        try:
            validate_error(status, raw)
        except ValueError:
            continue
        raise AssertionError('accepted wrong negative-case response')
    assert payload(MODELS[0], True)['provider']['allow_fallbacks'] is False
    print('offline PASS: structured JSON, SSE, 9 refusal controls; no network or credential reads')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--live', action='store_true')
    args = parser.parse_args()
    if not args.live:
        offline()
        return
    if os.environ.get('RECEIPTS_OPENROUTER_SMOKE') != '1':
        raise ValueError('explicit opt-in required')
    key = os.environ['OPENROUTER_API_KEY']
    signal.signal(signal.SIGALRM, lambda *_: (_ for _ in ()).throw(TimeoutError()))
    signal.alarm(75)
    for model in MODELS:
        for stream in (False, True):
            status, raw = exchange(payload(model, stream), key)
            validate(status, raw, stream)
            print(json.dumps(dict(model=model, stream=stream, verdict='PASS')))
    # Deliberately invalid model: one bounded error-path request, never retried.
    status, raw = exchange(payload('receipts/invalid-preflight-model', False), key)
    validate_error(status, raw)
    print('invalid-model error PASS')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        # Never print provider bodies, headers, exception text or secret values.
        print('preflight FAIL; stopped without retry', file=sys.stderr)
        sys.exit(1)
