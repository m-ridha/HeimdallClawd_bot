import contextlib
import io
import importlib.util
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('deploy_release', pathlib.Path(__file__).parents[1] / 'deploy-release.py')
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)
DEPLOYMENT = '00000000-0000-0000-0000-000000000001'
OTHER = '00000000-0000-0000-0000-000000000002'
SERVICE = '00000000-0000-0000-0000-000000000003'
ENV = {'RAILWAY_PROJECT_ID': SERVICE, 'RAILWAY_ENVIRONMENT_ID': SERVICE}

class DeploymentHealthTests(unittest.TestCase):
    def run_with(self, responses, *, github_actions=False):
        with patch.object(sys, 'argv', ['deploy-release.py', '.', SERVICE, 'v0.1.0']), patch.dict(os.environ, {**ENV, 'GITHUB_ACTIONS': 'true' if github_actions else 'false'}), patch.object(deploy, 'railway', side_effect=responses) as cli, patch.object(deploy.time, 'sleep') as sleep:
            deploy.main()
        return cli, sleep

    def test_another_healthy_deployment_does_not_finish_our_rollout(self):
        cli, sleep = self.run_with([
            {'deploymentId': DEPLOYMENT},
            [{'id': OTHER, 'status': 'SUCCESS'}, {'id': DEPLOYMENT, 'status': 'BUILDING'}],
            [{'id': DEPLOYMENT, 'status': 'SUCCESS'}],
        ])
        self.assertEqual(cli.call_count, 3)
        sleep.assert_called_once_with(15)
        self.assertIn('--detach', cli.call_args_list[0].args)
        self.assertIn('--json', cli.call_args_list[0].args)

    def test_progress_and_failures_do_not_print_identifiers(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.run_with([{'deploymentId': DEPLOYMENT}, [{'id': DEPLOYMENT, 'status': 'SUCCESS'}]])
        self.assertIn('agent: SUCCESS', output.getvalue())
        self.assertNotIn(SERVICE, output.getvalue())
        self.assertNotIn(DEPLOYMENT, output.getvalue())
        with self.assertRaises(RuntimeError) as error, contextlib.redirect_stdout(io.StringIO()):
            self.run_with([{'deploymentId': DEPLOYMENT}, [{'id': DEPLOYMENT, 'status': 'FAILED'}]])
        self.assertNotIn(DEPLOYMENT, str(error.exception))

    def test_actions_masks_dynamic_deployment_before_progress(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.run_with(
                [{'deploymentId': DEPLOYMENT}, [{'id': DEPLOYMENT, 'status': 'SUCCESS'}]],
                github_actions=True,
            )
        lines = output.getvalue().splitlines()
        self.assertEqual(lines[0], f'::add-mask::{DEPLOYMENT}')
        self.assertNotIn(DEPLOYMENT, '\n'.join(lines[1:]))
        self.assertNotIn(SERVICE, output.getvalue())

    def test_timeout_does_not_disclose_identifiers(self):
        with patch.object(deploy.time, 'monotonic', side_effect=[0, 901]), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, 'agent: deployment did not become healthy') as error:
                self.run_with([{'deploymentId': DEPLOYMENT}])
        self.assertNotIn(DEPLOYMENT, str(error.exception))

    def test_unexpected_api_status_is_not_logged(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            self.run_with([
                {'deploymentId': DEPLOYMENT},
                [{'id': DEPLOYMENT, 'status': 'TEST_private_response_payload'}],
                [{'id': DEPLOYMENT, 'status': 'SUCCESS'}],
            ])
        self.assertIn('agent: not yet listed', output.getvalue())
        self.assertNotIn('TEST_private_response_payload', output.getvalue())

    def test_failed_target_is_not_hidden_by_older_success(self):
        with self.assertRaisesRegex(RuntimeError, 'ended with FAILED'):
            self.run_with([
                {'deploymentId': DEPLOYMENT},
                [{'id': OTHER, 'status': 'SUCCESS'}, {'id': DEPLOYMENT, 'status': 'FAILED'}],
            ])

    def test_workflow_ancestry_check_uses_fully_qualified_remote_tracking_ref(self):
        build_workflow = (pathlib.Path(__file__).parents[2] / '.github/workflows/build.yml').read_text()
        self.assertNotIn('git merge-base --is-ancestor HEAD origin/main', build_workflow)
        ancestry_steps = [
            line.strip()
            for line in build_workflow.splitlines()
            if 'git merge-base --is-ancestor' in line
        ]
        self.assertEqual(len(ancestry_steps), 2)
        for step in ancestry_steps:
            self.assertEqual(step, 'run: git merge-base --is-ancestor HEAD refs/remotes/origin/main')

    def test_ancestry_check_rejects_tag_collision_for_unrelated_release_commit(self):
        with tempfile.TemporaryDirectory() as upstream_dir, tempfile.TemporaryDirectory() as client_dir:
            upstream = pathlib.Path(upstream_dir)
            client = pathlib.Path(client_dir)

            def git(args, cwd):
                return subprocess.run(
                    ['git', *args],
                    cwd=cwd,
                    capture_output=True,
                    text=True,
                    check=True,
                )

            # Initialize upstream with main branch
            git(['init', '-q'], cwd=upstream)
            git(['config', 'user.name', 'test'], cwd=upstream)
            git(['config', 'user.email', 'test@example.com'], cwd=upstream)
            (upstream / 'file.txt').write_text('main\n')
            git(['add', 'file.txt'], cwd=upstream)
            git(['commit', '-m', 'main commit'], cwd=upstream)
            git(['branch', '-M', 'main'], cwd=upstream)

            # Clone into client
            git(['clone', '-q', str(upstream), str(client)], cwd=client.parent)

            # Upstream creates an unrelated commit and a tag named origin/main
            git(['checkout', '-q', '--orphan', 'unrelated'], cwd=upstream)
            (upstream / 'unrelated.txt').write_text('unrelated\n')
            git(['add', 'unrelated.txt'], cwd=upstream)
            git(['commit', '-m', 'unrelated commit'], cwd=upstream)
            unrelated_sha = git(['rev-parse', 'HEAD'], cwd=upstream).stdout.strip()
            git(['tag', 'origin/main', unrelated_sha], cwd=upstream)

            # Client fetches all tags and checks out the unrelated commit
            git(['fetch', '-q', '--tags', 'origin'], cwd=client)
            git(['checkout', '-q', unrelated_sha], cwd=client)

            # Ambiguous ref `origin/main` would resolve to the tag and succeed (vulnerability)
            ambiguous = subprocess.run(
                ['git', 'merge-base', '--is-ancestor', 'HEAD', 'origin/main'],
                cwd=client,
                capture_output=True,
            )
            self.assertEqual(ambiguous.returncode, 0)

            # Fully-qualified ref `refs/remotes/origin/main` correctly rejects the unrelated commit
            mitigated = subprocess.run(
                ['git', 'merge-base', '--is-ancestor', 'HEAD', 'refs/remotes/origin/main'],
                cwd=client,
                capture_output=True,
            )
            self.assertNotEqual(mitigated.returncode, 0)

if __name__ == '__main__':
    unittest.main()
