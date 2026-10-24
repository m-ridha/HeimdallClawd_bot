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
            'volume.heimdall-data', 'volume.executor-workspace',
        })
        for service_name, volume_name, mount, size in [
            ('heimdallclawd', 'heimdall-data', '/data', 500),
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
                         {'containers': {'cpu': 2, 'memoryBytes': 1000000000}})
        self.assertEqual(resources['volume.executor-workspace']['config']['sizeMB'], 200)
