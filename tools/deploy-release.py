#!/usr/bin/env python3
"""Upload one service and wait for that exact deployment's healthy status."""
import json
import os
import pathlib
import subprocess
import sys
import time
import uuid


def railway(*args):
    result = subprocess.run(['railway', *args], capture_output=True, text=True, timeout=180)
    if result.returncode:
        raise RuntimeError('Railway command failed; inspect its dashboard logs (credentials suppressed)')
    return json.loads(result.stdout)


def exact_status(deployments, deployment_id):
    return next((item['status'] for item in deployments if item['id'] == deployment_id), None)


def main():
    path, service, tag = sys.argv[1:]
    for value in [service, os.environ['RAILWAY_PROJECT_ID'], os.environ['RAILWAY_ENVIRONMENT_ID']]:
        uuid.UUID(value)
    scope = ['--project', os.environ['RAILWAY_PROJECT_ID'], '--environment', os.environ['RAILWAY_ENVIRONMENT_ID'], '--service', service]
    uploaded = railway('up', str(pathlib.Path(path).resolve()), '--path-as-root', '--detach', '--json', '--message', f'HeimdallClawd {tag}', *scope)
    deployment_id = str(uuid.UUID(uploaded['deploymentId']))
    print(f'{service}: deployed {deployment_id}', flush=True)
    deadline = time.monotonic() + 900
    while time.monotonic() < deadline:
        status = exact_status(railway('deployment', 'list', '--json', '--limit', '50', *scope), deployment_id)
        print(f'{deployment_id}: {status or "not yet listed"}', flush=True)
        if status == 'SUCCESS':
            return
        if status in {'FAILED', 'CRASHED', 'REMOVED', 'SKIPPED'}:
            raise RuntimeError(f'Deployment {deployment_id} ended with {status}; no release will be published')
        time.sleep(15)
    raise RuntimeError(f'Deployment {deployment_id} did not become healthy within 15 minutes')


if __name__ == '__main__':
    main()
