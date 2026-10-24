#!/usr/bin/env python3
"""Verify the index, optionally every reachable commit, against a retired identifier."""
import argparse
import pathlib
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('identifier', help='Retired identifier, supplied by the operator')
parser.add_argument('--history', action='store_true')
args = parser.parse_args()
needle = args.identifier.encode().lower()
if not needle:
    parser.error('identifier must not be empty')

def git(*arguments):
    return subprocess.check_output(['git', *arguments])

problems = []
for raw in git('ls-files', '-z').split(b'\0'):
    if not raw:
        continue
    name = raw.decode()
    path = pathlib.PurePosixPath(name)
    data = git('show', ':' + name)
    if needle in raw.lower() or needle in data.lower():
        problems.append('retired identifier in indexed file: ' + name)
    if any(part in {'target', 'prebuilt'} for part in path.parts):
        problems.append('build/runtime artifact indexed: ' + name)
    if path.parts[0] == '.railway' and name not in {'.railway/railway.py', '.railway/README.md', '.railway/requirements.txt'}:
        problems.append('unexpected Railway state indexed: ' + name)
    if path.name in {'auth.json', 'auth-profiles.json', 'oauth_creds.json', '.secret_key', '.env'} or path.suffix in {'.db', '.db-wal', '.db-shm'}:
        problems.append('private state indexed: ' + name)
if args.history:
    for commit in git('rev-list', '--all').decode().splitlines():
        if needle in git('show', '-s', '--format=%B', commit).lower():
            problems.append('retired identifier in commit message: ' + commit)
        matches = subprocess.run(['git', 'grep', '-i', '-F', '-l', '-e', args.identifier, commit, '--'], capture_output=True)
        if matches.returncode not in {0, 1}:
            raise SystemExit('Cannot inspect reachable tree')
        for name in matches.stdout.decode().splitlines():
            problems.append('retired identifier in reachable tree: ' + name)
        for raw in git('ls-tree', '-r', '--name-only', '-z', commit).split(b'\0'):
            if raw and needle in raw.lower():
                problems.append('retired identifier in reachable path: ' + commit + ':' + raw.decode())
if problems:
    raise SystemExit('\n'.join(problems))
print('Clean index' + (' and reachable history' if args.history else '') + ' verified')
