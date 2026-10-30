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
    def package(self, temp, launcher_machine=b'\x3e\x00', runtime_machine=b'\x3e\x00'):
        inputs = []
        for name, machine in [('runtime', runtime_machine), ('launcher', launcher_machine)]:
            path = pathlib.Path(temp) / name
            content = bytearray(64)
            content[:6] = b'\x7fELF\x02\x01'
            content[18:20] = machine
            path.write_bytes(content)
            inputs.append(str(path))
        output = pathlib.Path(temp) / 'package'
        result = subprocess.run([sys.executable, str(ROOT / 'tools/package-release.py'), *inputs, str(output), 'v0.2.0', 'a' * 40], capture_output=True, text=True)
        return result, output

    def test_packages_verified_binaries_for_main_service(self):
        with tempfile.TemporaryDirectory() as temp:
            result, output = self.package(temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            lock = json.loads((output / 'deploy/bot/artifact.lock.json').read_text())
            for name, field in [('zeroclaw', 'binary_sha256'), ('heimdall-launcher', 'launcher_sha256')]:
                self.assertEqual(hashlib.sha256((output / 'prebuilt' / name).read_bytes()).hexdigest(), lock[field])
                self.assertIn(lock[field], (output / 'deploy/bot/runtime.sha256').read_text())
            dockerfile = (output / 'Dockerfile').read_text()
            self.assertIn('ENV PATH=/opt/heimdall-rust/bin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin CARGO_HOME=/tmp/cargo', dockerfile)
            self.assertIn('ENV HOME=/data/zeroclaw ZEROCLAW_CONFIG_DIR=/data/zeroclaw PORT=3888', dockerfile)
            self.assertNotRegex(dockerfile, r'\bcargo\s+(?:build|test|install|check)\b')
            self.assertIn('sha256sum -c', dockerfile)
            self.assertIn('visudo -cf /etc/sudoers.d/heimdall', dockerfile)
            self.assertIn('test "$(gosu heimdall id -u)" = 1000', dockerfile)
            self.assertNotIn('gosu heimdall sudo', dockerfile)
            workflow = (ROOT / '.github/workflows/build.yml').read_text()
            self.assertIn('gosu heimdall sudo -n cargo --version', workflow)

            self.assertEqual({key for key in lock if key.endswith('_patch_sha256')}, {
                'responses_tool_history_patch_sha256', 'sop_workshop_guidance_patch_sha256',
                'gateway_admin_auth_patch_sha256', 'http_diagnostics_patch_sha256',
                'http_secret_origins_patch_sha256', 'browser_output_patch_sha256',
                'private_telegram_patch_sha256', 'telegram_startup_patch_sha256',
                'tool_log_privacy_patch_sha256', 'security_dependencies_patch_sha256',
                'browser_navigation_patch_sha256', 'acp_pairing_revocation_patch_sha256',
            })
            self.assertEqual(lock['browser_output_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/browser-output.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['responses_tool_history_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/responses-tool-history.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['sop_workshop_guidance_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/sop-workshop-guidance.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['gateway_admin_auth_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/gateway-admin-auth.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['http_diagnostics_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/http-diagnostics.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['http_secret_origins_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/http-secret-origins.patch').read_bytes()).hexdigest())
            self.assertNotIn('webhook_timeout_patch_sha256', lock)
            self.assertNotIn('typed_transcription_patch_sha256', lock)
            self.assertNotIn('wasm_wit_patch_sha256', lock)
            self.assertFalse((output / 'launcher').exists())
            self.assertFalse((output / 'deploy/tests').exists())
            self.assertFalse((output / 'deploy/bot/patches').exists())
            self.assertFalse((output / 'deploy/bot/Dockerfile').exists())
            self.assertFalse((output / 'deploy/bot/Dockerfile.artifact').exists())
            self.assertFalse((output / 'deploy/bot/config.template.toml').exists())
            self.assertNotIn('executor_sha256', lock)
            self.assertNotIn('native_child_patch_sha256', lock)
            self.assertNotIn('remote_skills_patch_sha256', lock)
            for name in ['tool-log-privacy', 'security-dependencies', 'browser-navigation', 'acp-pairing-revocation']:
                field = name.replace('-', '_') + '_patch_sha256'
                self.assertEqual(lock[field], hashlib.sha256((ROOT / f'deploy/bot/patches/{name}.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['telegram_startup_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/telegram-startup.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['private_telegram_patch_sha256'], hashlib.sha256((ROOT / 'deploy/bot/patches/private-telegram.patch').read_bytes()).hexdigest())
            self.assertEqual(lock['features'], 'agent-runtime,tools-external,tool-project-intel,channel-telegram,gateway,schema-export,plugins-wasm-cranelift,embedded-web,webauthn,channel-acp-server,browser-native')
            self.assertIn('--features ' + lock['features'] + ' --bin zeroclaw', (ROOT / 'deploy/bot/Dockerfile').read_text())
            self.assertTrue((output / 'deploy/bot/evals/smoke.json').is_file())
            self.assertIn('COPY deploy/bot/evals /usr/share/heimdall/evals', (output / 'Dockerfile').read_text())
            self.assertFalse(list(output.rglob('railway.toml')))
            self.assertFalse((output / '.git').exists())
            self.assertFalse((output / 'launcher/target').exists())
            self.assertTrue((output / 'deploy/bot/entrypoint.sh').is_file())
            self.assertEqual({str(p.relative_to(output)) for p in output.rglob('*') if p.is_file()}, {
                '.dockerignore', 'Dockerfile', 'RELEASE_VERSION',
                'prebuilt/zeroclaw', 'prebuilt/heimdall-launcher',
                'deploy/bot/entrypoint.sh', 'deploy/bot/evals/smoke.json',
                'deploy/bot/LICENSE-ZEROCLAW-APACHE', 'deploy/bot/LICENSE-ZEROCLAW-MIT',
                'deploy/bot/NOTICE.md', 'deploy/bot/runtime.lock.json',
                'deploy/bot/runtime.sha256', 'deploy/bot/artifact.lock.json',
            })


    def test_unlisted_files_and_symlinks_are_not_packaged(self):
        # Run a synthetic checkout so no repository files are mutated.
        with tempfile.TemporaryDirectory() as temp:
            checkout = pathlib.Path(temp) / 'checkout'
            import shutil
            shutil.copytree(ROOT / 'tools', checkout / 'tools', ignore=shutil.ignore_patterns('__pycache__'))
            for name in ['deploy']:
                shutil.copytree(ROOT / name, checkout / name, ignore=shutil.ignore_patterns('target'))
            shutil.copyfile(ROOT / '.dockerignore', checkout / '.dockerignore')
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
            required = checkout / 'deploy/bot/entrypoint.sh'
            required.unlink()
            required.symlink_to(private / 'id_ed25519')
            with mock.patch.object(sys.modules[__name__], 'ROOT', checkout):
                result, _ = self.package(temp)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('must not contain symlinks', result.stderr)
            self.assertNotIn('TEST_private', result.stderr)
            required.unlink()
            required.write_text('synthetic entrypoint')
            patch = checkout / 'deploy/bot/patches/private-telegram.patch'
            patch.unlink()
            patch.symlink_to(private / 'id_ed25519')
            with mock.patch.object(sys.modules[__name__], 'ROOT', checkout):
                shutil.rmtree(output)
                result, _ = self.package(temp)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('must not contain symlinks', result.stderr)
            self.assertNotIn('TEST_private', result.stderr)


    def test_rejects_wrong_launcher_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            result, _ = self.package(temp, b'\xb7\x00')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Linux x86_64 launcher', result.stderr)

    def test_rejects_wrong_runtime_architecture(self):
        for component, expected in [('runtime', 'ELF')]:
            with self.subTest(component=component), tempfile.TemporaryDirectory() as temp:
                result, _ = self.package(temp, **{component + '_machine': b'\xb7\x00'})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn(f'Linux x86_64 {expected} executable', result.stderr)

if __name__ == '__main__':
    unittest.main()
