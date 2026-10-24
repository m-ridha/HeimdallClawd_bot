import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / 'verify-clean-tree.py'
REPO_ROOT = SCRIPT.parents[1]

class CleanTreeTests(unittest.TestCase):
    def test_iac_sources_are_allowed_but_state_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            subprocess.run(['git', 'init', '-q', directory], check=True)
            (root / '.railway').mkdir()
            for name in ['railway.py', 'README.md', 'requirements.txt']:
                (root / '.railway' / name).write_text('fixture\n')
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            result = subprocess.run([sys.executable, str(SCRIPT), 'retired-fixture'], cwd=root, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            (root / '.railway/state.json').write_text('{}')
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            result = subprocess.run([sys.executable, str(SCRIPT), 'retired-fixture'], cwd=root, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'unexpected Railway state', result.stderr)

    def test_private_oauth_credential_files_are_excluded_and_rejected(self):
        ignore_check = subprocess.run(
            ['git', 'check-ignore', '--no-index', 'oauth_creds.json', '.gemini/oauth_creds.json'],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        )
        self.assertEqual(ignore_check.returncode, 0, ignore_check.stderr)
        self.assertEqual(
            ignore_check.stdout.splitlines(),
            ['oauth_creds.json', '.gemini/oauth_creds.json'],
        )
        dockerignore = (REPO_ROOT / '.dockerignore').read_text().splitlines()
        self.assertIn('**/oauth_creds.json', dockerignore)

        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            subprocess.run(['git', 'init', '-q', directory], check=True)
            (root / '.gemini').mkdir()
            (root / '.gemini/oauth_creds.json').write_text('{}')
            subprocess.run(['git', 'add', '-f', '.gemini/oauth_creds.json'], cwd=root, check=True)
            result = subprocess.run([sys.executable, str(SCRIPT), 'retired-fixture'], cwd=root, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'private state indexed: .gemini/oauth_creds.json', result.stderr)

    def test_history_finds_removed_content(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            subprocess.run(['git', 'init', '-q', directory], check=True)
            (root / 'note.txt').write_text('retired-fixture\n')
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            commit = ['git', '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'snapshot']
            subprocess.run(commit, cwd=root, check=True)
            (root / 'note.txt').write_text('current\n')
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            subprocess.run(commit, cwd=root, check=True)
            result = subprocess.run([sys.executable, str(SCRIPT), 'retired-fixture', '--history'], cwd=root, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'retired identifier in reachable tree', result.stderr)
