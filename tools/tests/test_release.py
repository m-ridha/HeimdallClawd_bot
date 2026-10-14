import importlib.util
import os
import pathlib
import sys
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
    def run_with(self, responses):
        with patch.object(sys, 'argv', ['deploy-release.py', '.', SERVICE, 'v0.1.0']), patch.dict(os.environ, ENV), patch.object(deploy, 'railway', side_effect=responses) as cli, patch.object(deploy.time, 'sleep') as sleep:
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

    def test_failed_target_is_not_hidden_by_older_success(self):
        with self.assertRaisesRegex(RuntimeError, 'ended with FAILED'):
            self.run_with([
                {'deploymentId': DEPLOYMENT},
                [{'id': OTHER, 'status': 'SUCCESS'}, {'id': DEPLOYMENT, 'status': 'FAILED'}],
            ])

if __name__ == '__main__':
    unittest.main()
