#!/usr/bin/env python3
"""Package a runtime built from the pinned source; no credentials or Git data."""
import hashlib
import json
import pathlib
import re
import shutil
import sys

binary, destination, tag, commit = sys.argv[1:]
if not re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', tag):
    raise SystemExit('Invalid release tag')
if not re.fullmatch(r'[0-9a-f]{40}', commit):
    raise SystemExit('Invalid source commit')
binary_data = pathlib.Path(binary).read_bytes()
if binary_data[:6] != b'\x7fELF\x02\x01' or binary_data[18:20] != b'\x3e\x00':
    raise SystemExit('Expected a Linux x86_64 ELF executable')
root = pathlib.Path(__file__).resolve().parent.parent
out = pathlib.Path(destination)
out.mkdir(parents=True, exist_ok=False)
for name in ['launcher', 'deploy']:
    shutil.copytree(root / name, out / name, ignore=shutil.ignore_patterns('target', '.git', '.env*', '*.db*'))
(out / 'prebuilt').mkdir()
shutil.copyfile(binary, out / 'prebuilt/zeroclaw')
(out / 'prebuilt/zeroclaw').chmod(0o755)
digest = hashlib.sha256((out / 'prebuilt/zeroclaw').read_bytes()).hexdigest()
(out / 'deploy/bot/runtime.sha256').write_text(f'{digest}  /usr/local/bin/zeroclaw\n')
lock = json.loads((out / 'deploy/bot/runtime.lock.json').read_text())
provenance = {
    'release': tag, 'project_commit': commit,
    'runtime_revision': lock['revision'], 'source_sha256': lock['source_sha256'],
    'builder': 'rust:1.99-slim-bookworm, native Linux x86_64',
    'binary_sha256': digest,
    'private_telegram_patch_sha256': hashlib.sha256((out / 'deploy/bot/patches/private-telegram.patch').read_bytes()).hexdigest(),
}
(out / 'deploy/bot/artifact.lock.json').write_text(json.dumps(provenance, indent=2) + '\n')
shutil.copyfile(root / 'deploy/bot/Dockerfile.artifact', out / 'Dockerfile')
(out / 'railway.toml').write_text('''[build]
builder = "DOCKERFILE"
[deploy]
healthcheckPath = "/health"
healthcheckTimeout = 180
restartPolicyType = "ON_FAILURE"
restartPolicyMaxRetries = 3
''')
shutil.copyfile(root / '.dockerignore', out / '.dockerignore')
(out / 'RELEASE_VERSION').write_text(tag + '\n')
print(f'Packaged {tag} from {commit}; runtime SHA256 {digest}')
