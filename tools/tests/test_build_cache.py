"""Guard the native-test compiler cache against release-only inputs."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class NativeTestCacheTests(unittest.TestCase):
    def test_contract_compilation_is_stable_and_execution_is_still_required(self):
        dockerfile = (ROOT / 'deploy/bot/Dockerfile').read_text()
        compiled = dockerfile.split('FROM runtime-source AS native-test-compile\n', 1)[1].split('\nFROM ', 1)[0]
        executed = dockerfile.split('FROM native-test-compile AS runtime-build\n', 1)[1].split('\nFROM ', 1)[0]
        for volatile in ['COPY prebuilt/', '--from=launcher-validation', 'COPY deploy/bot/config.template.toml', 'COPY executor/entrypoint.sh']:
            self.assertNotIn(volatile, compiled)
            self.assertIn(volatile, executed)
        compile_commands = [line.strip().removeprefix('RUN ').removeprefix('&& ').removesuffix(' \\') for line in compiled.splitlines() if 'cargo test ' in line]
        run_commands = [line.strip().removeprefix('RUN ').removeprefix('&& ').removesuffix(' \\') for line in executed.splitlines() if 'cargo test ' in line]
        self.assertEqual(len(compile_commands), 5)
        self.assertIn('cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging --no-run', compile_commands)
        self.assertTrue(all(command.endswith(' --no-run') for command in compile_commands))
        self.assertEqual([command.removesuffix(' --no-run') for command in compile_commands], run_commands)
        self.assertIn('&& touch /contract-passed', executed)
        workflow = (ROOT / '.github/workflows/build.yml').read_text()
        self.assertIn('no-cache-filters: runtime-build', workflow)
        self.assertNotIn('no-cache: true', workflow)
