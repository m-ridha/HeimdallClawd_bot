#!/usr/bin/env bash
set -euo pipefail
project_dir=$(cd -- "$(dirname -- "$0")/../../.." && pwd)
output_dir=${1:?Usage: build-linux.sh OUTPUT_DIRECTORY}
mkdir -p -- "$output_dir"
output_dir=$(cd -- "$output_dir" && pwd)
build_dir=$(mktemp -d "${TMPDIR:-/tmp}/heimdall-build.XXXXXX")
trap 'rm -rf -- "$build_dir"' EXIT
read -r source_url source_sha < <(python3 - "$project_dir/deploy/bot/runtime.lock.json" <<'PY'
import json,sys
lock=json.load(open(sys.argv[1]))
print(lock['source_url'],lock['source_sha256'])
PY
)
curl --fail --location --retry 3 -H 'User-Agent: HeimdallClawd-build' "$source_url" -o "$build_dir/source.tar.gz"
python3 - "$build_dir/source.tar.gz" "$source_sha" <<'PY'
import hashlib,sys
assert hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest()==sys.argv[2], 'Source checksum mismatch'
PY
mkdir "$build_dir/source"
tar -xzf "$build_dir/source.tar.gz" --strip-components=1 -C "$build_dir/source"
for patch_file in "$project_dir"/deploy/bot/patches/*.patch; do
  patch -d "$build_dir/source" -p1 < "$patch_file"
done
cd "$build_dir/source"
features=$(python3 - "$project_dir/deploy/bot/runtime.lock.json" <<'PYFEATURES'
import json,sys
print(','.join(json.load(open(sys.argv[1]))['features']))
PYFEATURES
)
export CARGO_PROFILE_RELEASE_LTO=off CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_PROFILE_RELEASE_OPT_LEVEL=1
cargo zigbuild --release --locked --target x86_64-unknown-linux-gnu.2.36 --no-default-features --features "$features" --bin zeroclaw
target_dir=${CARGO_TARGET_DIR:-target}
binary="$target_dir/x86_64-unknown-linux-gnu/release/zeroclaw"
install -m 755 "$binary" "$output_dir/zeroclaw"
python3 - "$output_dir/zeroclaw" <<'PYHASH'
import hashlib,sys
print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest()+'  /usr/local/bin/zeroclaw')
PYHASH
