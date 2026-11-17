#!/usr/bin/env python3
"""Owned loopback TLS smart-HTTP and pinned SSH fixture; no external accounts."""
import argparse
import base64
import contextlib
import http.server
import json
import os
from pathlib import Path
import pwd
import secrets
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time


def run(args, **kwargs):
    kwargs.setdefault('cwd', '/')
    kwargs.setdefault('env', dict(PATH='/usr/bin:/bin', GIT_CONFIG_NOSYSTEM='1',
                                  GIT_CONFIG_GLOBAL='/dev/null', GIT_TERMINAL_PROMPT='0'))
    return subprocess.run(args, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=900, **kwargs).stdout


def config(path, entries):
    for key, value in entries:
        run(['git', 'config', '--file', str(path), key, str(value)])


class Backend(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass  # Request headers, bodies and credentials never enter evidence.

    def do_GET(self):
        self.serve()

    def do_POST(self):
        self.serve()

    def serve(self):
        if self.headers.get('Authorization') != self.server.authorization:
            self.send_response(401)
            self.send_header('WWW-Authenticate', 'Basic realm="fixture"')
            self.end_headers()
            return
        path, _, query = self.path.partition('?')
        if path not in ('/repo.git/info/refs', '/repo.git/git-upload-pack', '/repo.git/git-receive-pack'):
            self.send_error(404)
            return
        size = int(self.headers.get('Content-Length', '0'))
        if size > 1024 * 1024 or self.headers.get('Transfer-Encoding'):
            self.send_error(413)
            return
        env = dict(PATH='/usr/bin:/bin', GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null',
                   GIT_PROJECT_ROOT=str(self.server.root), GIT_HTTP_EXPORT_ALL='1',
                   PATH_INFO=path, QUERY_STRING=query, REQUEST_METHOD=self.command,
                   CONTENT_TYPE=self.headers.get('Content-Type', ''), CONTENT_LENGTH=str(size),
                   REMOTE_USER='fixture', REMOTE_ADDR='127.0.0.1')
        result = run([self.server.backend], input=self.rfile.read(size), env=env)
        headers, body = result.split(b'\r\n\r\n', 1)
        self.send_response(200)
        for line in headers.decode('ascii').split('\r\n'):
            key, value = line.split(':', 1)
            self.send_header(key, value.strip())
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def unused_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]


def fixture(test_binary, tls_only=False):
    os.umask(0o077)
    with tempfile.TemporaryDirectory(prefix='receipts-transport-', dir=Path.home()) as name, contextlib.ExitStack() as stack:
        root = Path(name).resolve()
        repo = root / 'repo.git'
        run(['git', 'init', '--bare', str(repo)])
        run(['git', '-C', str(repo), 'config', 'http.receivepack', 'true'])
        # Each certificate/key is generated here and destroyed with this private root.
        for cert in ('server', 'wrong'):
            run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1',
                 '-subj', '/CN=localhost', '-addext', 'subjectAltName=IP:127.0.0.1',
                 '-keyout', str(root / (cert + '.key')), '-out', str(root / (cert + '.crt'))])
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Backend)
        # Trust-refusal controls can reset TLS connections; never print handler diagnostics.
        server.handle_error = lambda *_: None
        stack.callback(server.server_close)
        server.root = root
        server.backend = str(Path(run(['git', '--exec-path']).decode().strip()) / 'git-http-backend')
        token = secrets.token_hex(24)
        server.authorization = 'Basic ' + base64.b64encode(('fixture:' + token).encode()).decode()
        tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        tls.load_cert_chain(root / 'server.crt', root / 'server.key')
        server.socket = tls.wrap_socket(server.socket, server_side=True)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        stack.callback(thread.join, 5)
        stack.callback(server.shutdown)
        helper = root / 'credential-helper'
        helper.write_text('#!/bin/sh\ncase "$1" in get) cat "' + str(root / 'credential') + '";; esac\n')
        helper.chmod(0o700)
        (root / 'credential').write_text('username=fixture\npassword=' + token + '\n')
        for label, ca, auth in [('good', 'server', helper), ('bad-trust', 'wrong', helper),
                                ('bad-auth', 'server', '/usr/bin/false')]:
            config(root / ('https-' + label), [('http.sslVerify', 'true'),
                   ('http.sslCAInfo', root / (ca + '.crt')), ('http.followRedirects', 'false'),
                   ('credential.helper', auth), ('credential.useHttpPath', 'true')])
        endpoints = [('https', f'https://127.0.0.1:{server.server_port}/repo.git')]
        if not tls_only:
            port = unused_port()
            for key in ('host', 'wrong-host', 'client', 'wrong-client'):
                run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / key)])
            (root / 'authorized_keys').write_bytes((root / 'client.pub').read_bytes())
            for label, key in [('good', 'host'), ('bad-trust', 'wrong-host')]:
                (root / ('known-' + label)).write_text(
                    f'[127.0.0.1]:{port} ' + (root / (key + '.pub')).read_text())
            user = pwd.getpwuid(os.getuid()).pw_name
            sshd_config = root / 'sshd_config'
            sshd_config.write_text(f'''ListenAddress 127.0.0.1
Port {port}
HostKey {root}/host
PidFile {root}/sshd.pid
AuthorizedKeysFile {root}/authorized_keys
StrictModes yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
PermitRootLogin prohibit-password
AllowUsers {user}
AllowTcpForwarding no
X11Forwarding no
PermitTunnel no
''')
            sshd = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(sshd_config)],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env={'PATH': '/usr/bin:/bin'})
            def stop():
                if sshd.poll() is None:
                    sshd.terminate()
                sshd.wait(timeout=10)
            stack.callback(stop)
            for _ in range(50):
                if sshd.poll() is not None:
                    raise RuntimeError('owned sshd failed to start')
                try:
                    with socket.create_connection(('127.0.0.1', port), timeout=.1):
                        break
                except OSError:
                    time.sleep(.1)
            else:
                raise RuntimeError('owned sshd unavailable')
            for label, host, key in [('good', 'good', 'client'), ('bad-trust', 'bad-trust', 'client'),
                                      ('bad-auth', 'good', 'wrong-client')]:
                command = (f'/usr/bin/ssh -F /dev/null -o BatchMode=yes -o IdentitiesOnly=yes '
                           f'-o IdentityAgent=none -o StrictHostKeyChecking=yes -o GlobalKnownHostsFile=/dev/null '
                           f'-o UserKnownHostsFile={root}/known-{host} -i {root}/{key} -o ConnectTimeout=5')
                config(root / ('ssh-' + label), [('core.sshCommand', command)])
            endpoints.append(('ssh', f'ssh://{user}@127.0.0.1:{port}{repo}'))
        for kind, endpoint in endpoints:
            env = dict(PATH='/usr/bin:/bin', RECEIPTS_TRANSPORT_FIXTURE=str(root),
                       RECEIPTS_TRANSPORT_ENDPOINT=endpoint, RECEIPTS_TRANSPORT_KIND=kind)
            # Captured output is test diagnostics only; never upload arbitrary process output.
            result = subprocess.run([str(test_binary), 'remote_publish_tests::owned_network_qualification',
                                     '--ignored', '--exact', '--test-threads=1'], env=env,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
            if result.returncode or b'1 passed; 0 failed' not in result.stdout:
                raise RuntimeError(f'{kind} durable fixture failed (exit {result.returncode})')
            published = run(['git', '-C', str(repo), 'rev-parse', 'refs/heads/task/transport']).decode().strip()
            run(['git', '-C', str(repo), 'update-ref', '-d', 'refs/heads/task/transport', published])
            assert not run(['git', '-C', str(repo), 'for-each-ref'])
            print(json.dumps(dict(transport=kind, durable_publication='PASS', exact_readback='PASS',
                                  trust_refusal='PASS', auth_refusal='PASS', replay='PASS',
                                  policy_refusal='PASS', non_fast_forward='PASS')), flush=True)
    print('owned process shutdown and fixture cleanup PASS')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('test_binary', type=Path)
    parser.add_argument('--tls-only', action='store_true', help='partial local control; no SSH qualification')
    args = parser.parse_args()
    try:
        fixture(args.test_binary.resolve(strict=True), args.tls_only)
    except Exception:
        raise SystemExit('transport qualification FAIL; no credential diagnostics retained') from None
