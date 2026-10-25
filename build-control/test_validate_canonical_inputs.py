"""The offline guard must allow SSL imports and deny socket aliases too."""
import subprocess
import sys
import unittest
from pathlib import Path


class OfflineGuardTest(unittest.TestCase):
    def test_ssl_import_and_socket_denials_in_fresh_process(self):
        checker = Path(__file__).with_name("validate_canonical_inputs.py")
        guard = checker.read_text().split("\nroot = Path", 1)[0]
        probe = guard + '''
import socket
import ssl
for operation in (socket.socket, socket.SocketType,
                  lambda: socket.getaddrinfo("127.0.0.1", 80)):
    try:
        result = operation()
    except RuntimeError as error:
        assert str(error) == "canonical validation is offline"
    else:
        if isinstance(result, socket.SocketType):
            result.close()
        raise AssertionError("socket operation was allowed")
'''
        result = subprocess.run(
            [sys.executable, "-I", "-B", "-c", probe],
            capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
