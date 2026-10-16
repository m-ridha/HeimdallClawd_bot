import pathlib
import subprocess
import sys
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / 'verify-clean-tree.py'

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
