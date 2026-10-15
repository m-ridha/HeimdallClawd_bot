import importlib.util
import pathlib
import unittest
import io
import urllib.error
from unittest.mock import patch

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

    def test_http_request_has_application_user_agent(self):
        payload = b'{"data":{"projectToken":{"project":{"id":"project"},"environment":{"id":"production"}}}}'
        with patch.dict(scope.os.environ, {'RAILWAY_TOKEN': 'test-only-token', 'RAILWAY_PROJECT_ID': 'project', 'RAILWAY_ENVIRONMENT_ID': 'production'}), patch.object(scope.urllib.request, 'build_opener') as opener:
            opener.return_value.open.return_value.__enter__.return_value = io.BytesIO(payload)
            scope.main()
            request = opener.return_value.open.call_args.args[0]
            self.assertEqual(request.get_header('User-agent'), 'HeimdallClawd-CI')
            self.assertEqual(request.get_header('Project-access-token'), 'test-only-token')

    def test_http_failure_preserves_only_status(self):
        with patch.dict(scope.os.environ, {'RAILWAY_TOKEN': 'test-only-token'}), patch.object(scope.urllib.request, 'build_opener') as opener:
            opener.return_value.open.side_effect = urllib.error.HTTPError('https://example.invalid', 403, 'sensitive response', {}, None)
            with self.assertRaisesRegex(RuntimeError, 'HTTP 403') as error:
                scope.main()
            self.assertNotIn('sensitive response', str(error.exception))
