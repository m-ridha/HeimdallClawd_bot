#!/bin/sh
set -eu
umask 077
install -d -o 64173 -g 64173 -m 700 /data/workspace
install -d -o 0 -g 0 -m 711 /run/heimdall-tmp
install -d -m 700 /run/heimdall-control
printf 'protected probe fixture\n' > /run/heimdall-control/probe
exec /usr/local/bin/heimdall-executor
