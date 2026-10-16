import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]

class PackageTests(unittest.TestCase):
    def package(self, temp, launcher_machine=b'\x3e\x00'):
        inputs = []
        for name, machine in [('runtime', b'\x3e\x00'), ('launcher', launcher_machine), ('executor', b'\x3e\x00')]:
            path = pathlib.Path(temp) / name
            content = bytearray(64)
            content[:6] = b'\x7fELF\x02\x01'
            content[18:20] = machine
            path.write_bytes(content)
            inputs.append(str(path))
        output = pathlib.Path(temp) / 'package'
        result = subprocess.run([sys.executable, str(ROOT / 'tools/package-release.py'), *inputs, str(output), 'v0.2.0', 'a' * 40], capture_output=True, text=True)
        return result, output

    def test_packages_verified_binaries_for_both_services(self):
        with tempfile.TemporaryDirectory() as temp:
            result, output = self.package(temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            lock = json.loads((output / 'deploy/bot/artifact.lock.json').read_text())
            for name, field in [('zeroclaw', 'binary_sha256'), ('heimdall-launcher', 'launcher_sha256')]:
                self.assertEqual(hashlib.sha256((output / 'prebuilt' / name).read_bytes()).hexdigest(), lock[field])
                self.assertIn(lock[field], (output / 'deploy/bot/runtime.sha256').read_text())
            self.assertEqual(hashlib.sha256((output / 'executor/heimdall-executor').read_bytes()).hexdigest(), lock['executor_sha256'])
            for dockerfile in [output / 'Dockerfile', output / 'executor/Dockerfile']:
                self.assertNotIn('cargo ', dockerfile.read_text())
                self.assertIn('sha256sum -c', dockerfile.read_text())
            self.assertEqual(lock['features'], 'agent-runtime,channel-telegram,gateway,schema-export,plugins-wasm-cranelift,embedded-web,webauthn,channel-acp-server')
            self.assertIn('--features ' + lock['features'] + ' --bin zeroclaw', (ROOT / 'deploy/bot/Dockerfile').read_text())
            self.assertTrue((output / 'deploy/bot/evals/smoke.json').is_file())
            self.assertIn('COPY deploy/bot/evals /usr/share/heimdall/evals', (output / 'Dockerfile').read_text())
            self.assertFalse(list(output.rglob('railway.toml')))
            self.assertFalse((output / '.git').exists())
            self.assertFalse((output / 'launcher/target').exists())

    def test_rejects_wrong_launcher_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            result, _ = self.package(temp, b'\xb7\x00')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Linux x86_64 launcher', result.stderr)

if __name__ == '__main__':
    unittest.main()
