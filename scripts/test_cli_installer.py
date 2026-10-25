import functools
import hashlib
import http.server
import io
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import threading
import unittest

INSTALLER = Path(__file__).resolve().parents[1] / 'src-tauri/crates/cairn-registry/assets/install.sh'

class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *_args):
        pass

class InstallerAcceptance(unittest.TestCase):
    def test_verified_install_and_failed_replacement_preserve_current_commands(self):
        with tempfile.TemporaryDirectory(prefix='cairn-installer-') as temporary:
            root = Path(temporary)
            public = root / 'public'
            public.mkdir()
            prefix = root / 'installed'
            system = {'Darwin': 'darwin', 'Linux': 'linux'}[platform.system()]
            arch = {'arm64': 'arm64', 'aarch64': 'arm64', 'x86_64': 'x86_64'}[platform.machine()]
            filename = f'ecky-cli-{system}-{arch}.tar.gz'
            def release(version, corrupt=False, unsafe=False, extra_resource=False):
                directory = public / 'cli' / version
                directory.mkdir(parents=True, exist_ok=True)
                archive = directory / filename
                with tarfile.open(archive, 'w:gz') as writer:
                    for name in ['ecky', 'cairn']:
                        body = f'#!/bin/sh\nprintf "%s\\n" "{name} {version}"\n'.encode()
                        entry = tarfile.TarInfo(f'ecky-cli/bin/{name}')
                        entry.mode = 0o755
                        entry.size = len(body)
                        writer.addfile(entry, io.BytesIO(body))
                    if extra_resource:
                        entry = tarfile.TarInfo('ecky-cli/resources/changed.txt')
                        entry.size = 1
                        writer.addfile(entry, io.BytesIO(b'x'))
                    if unsafe:
                        entry = tarfile.TarInfo('../escape')
                        entry.size = 1
                        writer.addfile(entry, io.BytesIO(b'x'))
                digest = '0' * 64 if corrupt else hashlib.sha256(archive.read_bytes()).hexdigest()
                (directory / 'SHA256SUMS').write_text(f'{digest}  {filename}\n')
                (public / 'cli' / 'latest.txt').write_text(version + '\n')
            handler = functools.partial(QuietHandler, directory=str(public))
            server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            env = {**os.environ, 'CAIRN_HUB': f'http://127.0.0.1:{server.server_port}', 'CAIRN_PREFIX': str(prefix)}
            try:
                release('v1')
                installed = subprocess.run(['sh', str(INSTALLER)], env=env, text=True, capture_output=True)
                self.assertEqual(installed.returncode, 0, installed.stderr)
                original = os.readlink(prefix / 'lib/ecky-cli/current')
                self.assertEqual(subprocess.check_output([prefix / 'bin/cairn'], text=True).strip(), 'cairn v1')
                release('v1', extra_resource=True)
                changed = subprocess.run(['sh', str(INSTALLER)], env=env, capture_output=True)
                self.assertNotEqual(changed.returncode, 0, 'same release with changed resources must be rejected')
                self.assertEqual(os.readlink(prefix / 'lib/ecky-cli/current'), original)
                release('v2', corrupt=True)
                failed = subprocess.run(['sh', str(INSTALLER)], env=env, text=True, capture_output=True)
                self.assertNotEqual(failed.returncode, 0)
                self.assertEqual(os.readlink(prefix / 'lib/ecky-cli/current'), original)
                self.assertEqual(subprocess.check_output([prefix / 'bin/ecky'], text=True).strip(), 'ecky v1')
                release('v3', unsafe=True)
                self.assertNotEqual(subprocess.run(['sh', str(INSTALLER)], env=env, capture_output=True).returncode, 0)
                self.assertEqual(os.readlink(prefix / 'lib/ecky-cli/current'), original)
            finally:
                server.shutdown()
                server.server_close()
                thread.join()

if __name__ == '__main__':
    unittest.main()
