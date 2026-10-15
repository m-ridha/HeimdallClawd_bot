import importlib.util
import pathlib
import unittest

spec = importlib.util.spec_from_file_location('token_scope', pathlib.Path(__file__).parents[1] / 'verify-railway-token.py')
scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scope)


class TokenScopeTests(unittest.TestCase):
    def test_accepts_matching_scope(self):
        scope.validate_scope({'data': {'projectToken': {'project': {'id': 'project'}, 'environment': {'id': 'production'}}}}, 'project', 'production')

    def test_rejects_different_environment(self):
        with self.assertRaisesRegex(RuntimeError, 'does not match'):
            scope.validate_scope({'data': {'projectToken': {'project': {'id': 'project'}, 'environment': {'id': 'staging'}}}}, 'project', 'production')

    def test_auth_errors_are_sanitized(self):
        with self.assertRaisesRegex(RuntimeError, 'authentication failed') as error:
            scope.validate_scope({'errors': [{'message': 'sensitive response'}]}, 'project', 'production')
        self.assertNotIn('sensitive response', str(error.exception))
