"""Guard the native-test compiler cache against release-only inputs."""
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]


class NativeTestCacheTests(unittest.TestCase):
    def test_contract_compilation_is_stable_and_execution_is_still_required(self):
        dockerfile = (ROOT / 'deploy/bot/Dockerfile').read_text()
        compiled = dockerfile.split('FROM runtime-source AS native-test-compile\n', 1)[1].split('\nFROM ', 1)[0]
        executed = dockerfile.split('FROM native-test-compile AS runtime-build\n', 1)[1].split('\nFROM ', 1)[0]
        for volatile in ['COPY prebuilt/', '--from=launcher-validation', 'COPY deploy/bot/config.template.toml']:
            self.assertNotIn(volatile, compiled)
            self.assertIn(volatile, executed)
        compile_commands = [line.strip().removeprefix('RUN ').removeprefix('&& ').removesuffix(' \\') for line in compiled.splitlines() if 'cargo test ' in line]
        run_commands = [line.strip().removeprefix('RUN ').removeprefix('&& ').removesuffix(' \\') for line in executed.splitlines() if 'cargo test ' in line]
        self.assertEqual(len(compile_commands), 12)
        self.assertIn('cargo test --locked -p zeroclaw-runtime --no-default-features --features tools-external,tool-project-intel,browser-native --lib heimdall_read_skill --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-log --test heimdall_tool_privacy --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-runtime --no-default-features --features observability-otel --lib heimdall_otel --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-providers --lib heimdall --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-gateway --no-default-features --lib heimdall --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram,channel-acp-server --lib heimdall --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --lib heimdall --no-run', compile_commands)
        self.assertIn('cargo test --locked -p zeroclaw-channels --no-default-features --features channel-telegram --test heimdall_logging --test heimdall_transcription --no-run', compile_commands)
        self.assertIn('COPY deploy/tests/browser-output.rs crates/zeroclaw-tools/tests/heimdall_browser_output.rs', compiled)
        self.assertIn('COPY deploy/tests/browser-navigation.rs crates/zeroclaw-tools/tests/heimdall_browser_navigation.rs', compiled)
        self.assertTrue(any('--test heimdall_browser_navigation' in command for command in compile_commands))
        self.assertIn('cargo test --locked -p zeroclaw-tools --no-default-features --features browser-native --lib heimdall --no-run', compile_commands)
        self.assertTrue(any('--test heimdall_browser_output' in command for command in compile_commands))
        self.assertTrue(all(command.endswith(' --no-run') for command in compile_commands))
        self.assertEqual([command.removesuffix(' --no-run') for command in compile_commands], run_commands)
        self.assertIn('&& touch /contract-passed', executed)
        workflow = (ROOT / '.github/workflows/build.yml').read_text()
        self.assertIn('no-cache-filters: runtime-build', workflow)
        self.assertNotIn('no-cache: true', workflow)

    def test_eager_skill_patch_is_applied_before_native_contract_compilation(self):
        dockerfile = (ROOT / 'deploy/bot/Dockerfile').read_text()
        # Inspect runtime-source explicitly: applying a patch in a later stage
        # would leave the cached native test binaries testing unpatched code.
        source = dockerfile.split(' AS runtime-source\n', 1)[1].split('\nFROM ', 1)[0]
        copy = 'COPY deploy/bot/patches/eager-read-skill.patch /tmp/eager-read-skill.patch'
        apply = 'patch --fuzz=0 -p1 < /tmp/eager-read-skill.patch'
        self.assertEqual(source.count(copy), 1)
        self.assertEqual(source.count(apply), 1)
        self.assertLess(source.index(copy), source.index(apply))
        self.assertIn('FROM runtime-source AS native-test-compile', dockerfile)
        self.assertIn('eager-read-skill.patch', (ROOT / 'deploy/bot/NOTICE.md').read_text())
