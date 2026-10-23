#!/usr/bin/env python3
"""Package a runtime built from the pinned source; no credentials or Git data."""
import hashlib
import json
import pathlib
import re
import shutil
import sys


def read_linux_binary(path, component):
    data = pathlib.Path(path).read_bytes()
    if data[:6] != b'\x7fELF\x02\x01' or data[18:20] != b'\x3e\x00':
        raise SystemExit(f'Expected a Linux x86_64 {component} executable')
    return data


def source_file(root, name):
    relative = pathlib.PurePosixPath(name)
    if not relative.parts or relative.is_absolute() or '..' in relative.parts or (relative.parts[0] != 'deploy' and name != '.dockerignore'):
        raise SystemExit('Invalid release input path')
    source = root
    for part in relative.parts:
        source = source / part
        if source.is_symlink():
            raise SystemExit('Release inputs must not contain symlinks')
    if not source.is_file():
        raise SystemExit('Required release input is missing')
    return source


binary, launcher_binary, destination, tag, commit = sys.argv[1:]
if not re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', tag):
    raise SystemExit('Invalid release tag')
if not re.fullmatch(r'[0-9a-f]{40}', commit):
    raise SystemExit('Invalid source commit')
binary_data = read_linux_binary(binary, 'ELF')
root = pathlib.Path(__file__).resolve().parent.parent
out = pathlib.Path(destination)
out.mkdir(parents=True, exist_ok=False)
# Copy reviewed files only. Neither gitignore nor Docker ignores protect a tarball.
manifest = root / 'tools/release-files.txt'
paths = [line for line in manifest.read_text().splitlines() if line and not line.startswith('#')]
if len(paths) != len(set(paths)):
    raise SystemExit('Duplicate release manifest entry')
for name in paths:
    source = source_file(root, name)
    # The final image recipe has one canonical deployed path.
    relative = pathlib.PurePosixPath('Dockerfile' if name == 'deploy/bot/Dockerfile.artifact' else name)
    target = out / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
(out / 'prebuilt').mkdir()
(out / 'prebuilt/zeroclaw').write_bytes(binary_data)
(out / 'prebuilt/zeroclaw').chmod(0o755)
digest = hashlib.sha256(binary_data).hexdigest()
launcher_data = read_linux_binary(launcher_binary, 'launcher')
(out / 'prebuilt/heimdall-launcher').write_bytes(launcher_data)
(out / 'prebuilt/heimdall-launcher').chmod(0o755)
launcher_digest = hashlib.sha256(launcher_data).hexdigest()
(out / 'deploy/bot/runtime.sha256').write_text(f'{digest}  /usr/local/bin/zeroclaw\n{launcher_digest}  /usr/local/bin/heimdall-launcher\n')
lock = json.loads((out / 'deploy/bot/runtime.lock.json').read_text())
provenance = {
    'release': tag, 'project_commit': commit,
    'runtime_revision': lock['revision'], 'source_sha256': lock['source_sha256'],
    'builder': 'rust:1.99-slim-bookworm, native Linux x86_64',
    'binary_sha256': digest, 'launcher_sha256': launcher_digest,
    'features': ','.join(lock['features']),
    'profile': 'release: opt-level=1, lto=off, codegen-units=16',
    'responses_tool_history_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/responses-tool-history.patch').read_bytes()).hexdigest(),
    'sop_workshop_guidance_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/sop-workshop-guidance.patch').read_bytes()).hexdigest(),
    'gateway_admin_auth_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/gateway-admin-auth.patch').read_bytes()).hexdigest(),
    'http_diagnostics_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/http-diagnostics.patch').read_bytes()).hexdigest(),
    'http_secret_origins_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/http-secret-origins.patch').read_bytes()).hexdigest(),
    'private_telegram_patch_sha256': hashlib.sha256(source_file(root, 'deploy/bot/patches/private-telegram.patch').read_bytes()).hexdigest(),
}
(out / 'deploy/bot/artifact.lock.json').write_text(json.dumps(provenance, indent=2) + '\n')
(out / 'RELEASE_VERSION').write_text(tag + '\n')
print(f'Packaged {tag} from {commit}; runtime SHA256 {digest}')
