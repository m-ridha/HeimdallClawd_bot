"""Guard persistent state and unmanaged credentials during infrastructure changes."""
import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]

@unittest.skipUnless(importlib.util.find_spec('railway_sdk'), 'requires the pinned Python IaC SDK')
class InfrastructureSafetyTests(unittest.TestCase):
    def test_existing_volumes_and_credentials_remain_outside_release_artifacts(self):
        spec = importlib.util.spec_from_file_location('heimdall_infra', ROOT / '.railway/railway.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        graph = module.main().to_graph()
        self.assertEqual(graph['variables'], {'managed': False})
        resources = {resource['address']: resource for resource in graph['resources']}
        self.assertEqual(set(resources), {
            'service.heimdallclawd',
            'volume.heimdall-data',
        })
        for service_name, volume_name, mount, size in [
            ('heimdallclawd', 'heimdall-data', '/data', 5000),
        ]:
            service = resources['service.' + service_name]
            self.assertNotIn('variables', service)
            self.assertNotIn('source', service)
            self.assertEqual(service['kind'], 'empty')
            attachment = service['volumeAttachments'][volume_name]
            self.assertEqual(attachment['volume'], 'volume.' + volume_name)
            self.assertEqual(attachment['mountPath'], mount)
            volume = resources['volume.' + volume_name]
            self.assertEqual(volume['config']['sizeMB'], size)
            self.assertEqual(volume['config']['region'], 'asia-southeast1-eqsg3a')
            self.assertEqual(service['deploy']['healthcheckPath'], '/health')
            self.assertEqual(service['deploy']['multiRegionConfig'], {'asia-southeast1-eqsg3a': {'numReplicas': 1}})

        self.assertEqual(resources['service.heimdallclawd']['deploy']['limitOverride'],
                         {'containers': {'cpu': 8, 'memoryBytes': 8000000000}})

    def test_resized_service_has_exactly_one_persistent_mount(self):
        spec = importlib.util.spec_from_file_location('heimdall_infra', ROOT / '.railway/railway.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        resources = {resource['address']: resource for resource in module.main().to_graph()['resources']}
        attachments = resources['service.heimdallclawd']['volumeAttachments']
        self.assertEqual(set(attachments), {'heimdall-data'})
        self.assertEqual(attachments['heimdall-data']['mountPath'], '/data')
        self.assertEqual(attachments['heimdall-data']['volume'], 'volume.heimdall-data')
        self.assertNotIn('volume.executor-workspace', resources)

    def test_deployment_limits_preserve_explicit_cpu_and_byte_units(self):
        spec = importlib.util.spec_from_file_location('heimdall_infra', ROOT / '.railway/railway.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        for cpu, memory in [(8, 8_000_000_000), (1, 512_000_000)]:
            with self.subTest(cpu=cpu, memory=memory):
                self.assertEqual(module.deployment(cpu, memory), {
                    'restartPolicyMaxRetries': 3,
                    'drainingSeconds': 0,
                    'overlapSeconds': 0,
                    'limitOverride': {'containers': {'cpu': cpu, 'memoryBytes': memory}},
                })
