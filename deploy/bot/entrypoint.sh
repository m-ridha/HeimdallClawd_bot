#!/bin/bash
# tini forwards signals to this entire process group and reaps orphaned children.
set -euo pipefail
if [[ ${HEIMDALL_RECOVERY:-0} == 1 ]]; then
    exec /usr/local/bin/heimdall-launcher "$@"
fi
# No provider credentials are needed by the loopback-only WebDriver.
gosu heimdall env -i PATH=/usr/bin:/bin HOME=/tmp \
    chromedriver --port=9515 --allowed-ips=127.0.0.1,::1 \
    >/dev/null 2>&1 &
driver=$!
/usr/local/bin/heimdall-launcher "$@" &
agent=$!
cleanup() {
    kill "$agent" "$driver" 2>/dev/null || true
    wait "$agent" "$driver" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
status=0
wait -n "$driver" "$agent" || status=$?
exit "$status"
