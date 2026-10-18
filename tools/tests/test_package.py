import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

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
                self.assertNotRegex(dockerfile.read_text(), r'\bcargo\s+(?:build|test|install|check)\b')
                self.assertIn('sha256sum -c', dockerfile.read_text())
            self.assertEqual(lock['native_child_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/native-child.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['http_diagnostics_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/http-diagnostics.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['http_secret_origins_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/http-secret-origins.patch').read_bytes()).hexdigest())
            self.assertNotIn('typed_transcription_patch_sha256', lock)
            self.assertNotIn('wasm_wit_patch_sha256', lock)
            self.assertEqual({p.name for p in (output / 'deploy/bot/patches').iterdir()}, {
                'native-child.patch', 'http-secret-origins.patch', 'http-diagnostics.patch',
                'private-telegram.patch', 'remote-skills.patch',
            })
            self.assertEqual(lock['features'], 'agent-runtime,tools-external,tool-project-intel,channel-telegram,gateway,schema-export,plugins-wasm-cranelift,embedded-web,webauthn,channel-acp-server')
            self.assertIn('--features ' + lock['features'] + ' --bin zeroclaw', (ROOT / 'deploy/bot/Dockerfile').read_text())
            self.assertTrue((output / 'deploy/bot/evals/smoke.json').is_file())
            self.assertIn('COPY deploy/bot/evals /usr/share/heimdall/evals', (output / 'Dockerfile').read_text())
            self.assertFalse(list(output.rglob('railway.toml')))
            self.assertFalse((output / '.git').exists())
            self.assertFalse((output / 'launcher/target').exists())

    def test_unlisted_files_and_symlinks_are_not_packaged(self):
        # Run a synthetic checkout so no repository files are mutated.
        with tempfile.TemporaryDirectory() as temp:
            checkout = pathlib.Path(temp) / 'checkout'
            import shutil
            shutil.copytree(ROOT / 'tools', checkout / 'tools', ignore=shutil.ignore_patterns('__pycache__'))
            for name in ['launcher', 'deploy']:
                shutil.copytree(ROOT / name, checkout / name, ignore=shutil.ignore_patterns('target'))
            shutil.copyfile(ROOT / '.dockerignore', checkout / '.dockerignore')
            (checkout / 'executor').mkdir()
            shutil.copyfile(ROOT / 'executor/entrypoint.sh', checkout / 'executor/entrypoint.sh')
            private = checkout / 'deploy/bot/.ssh'
            private.mkdir()
            (private / 'id_ed25519').write_text('TEST_private_credential')
            (checkout / 'deploy/bot/auth.json').write_text('TEST_private_auth')
            (checkout / 'deploy/bot/extra-link').symlink_to(private)
            with mock.patch.object(sys.modules[__name__], 'ROOT', checkout):
                result, output = self.package(temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((output / 'deploy/bot/.ssh').exists())
            self.assertFalse((output / 'deploy/bot/auth.json').exists())
            self.assertFalse((output / 'deploy/bot/extra-link').exists())
            shutil.rmtree(output)
            required = checkout / 'deploy/bot/config.template.toml'
            required.unlink()
            required.symlink_to(private / 'id_ed25519')
            with mock.patch.object(sys.modules[__name__], 'ROOT', checkout):
                result, _ = self.package(temp)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('must not contain symlinks', result.stderr)
            self.assertNotIn('TEST_private', result.stderr)

    def test_rejects_wrong_launcher_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            result, _ = self.package(temp, b'\xb7\x00')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Linux x86_64 launcher', result.stderr)

if __name__ == '__main__':
    unittest.main()
