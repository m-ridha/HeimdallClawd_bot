#!/usr/bin/env python3
"""Read only the project token's scope; never print credentials or API responses."""
import json
import os
import urllib.error
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def validate_scope(response, project_id, environment_id):
    scope = (response.get('data') or {}).get('projectToken')
    if response.get('errors') or not isinstance(scope, dict):
        raise RuntimeError('Railway project-token authentication failed; check the production RAILWAY_TOKEN secret')
    if (scope.get('project') or {}).get('id') != project_id or (scope.get('environment') or {}).get('id') != environment_id:
        raise RuntimeError('Railway token scope does not match the production project/environment variables')


def main():
    request = urllib.request.Request(
        'https://backboard.railway.com/graphql/v2',
        data=json.dumps({'query': 'query { projectToken { project { id } environment { id } } }'}).encode(),
        headers={
            'Content-Type': 'application/json',
            'User-Agent': 'HeimdallClawd-CI',
            'project-access-token': os.environ['RAILWAY_TOKEN'],
        },
    )
    try:
        with urllib.request.build_opener(NoRedirect).open(request, timeout=45) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f'Railway token scope check received HTTP {error.code}; API response suppressed') from None
    except urllib.error.URLError:
        raise RuntimeError('Railway token scope check could not reach the API; connection details suppressed') from None
    except ValueError:
        raise RuntimeError('Railway token scope check received invalid JSON; API response suppressed') from None
    validate_scope(result, os.environ['RAILWAY_PROJECT_ID'], os.environ['RAILWAY_ENVIRONMENT_ID'])
    print('Railway project token matches the production project and environment')


if __name__ == '__main__':
    try:
        main()
    except RuntimeError as error:
        raise SystemExit(str(error)) from None
