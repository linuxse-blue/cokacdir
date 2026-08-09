#!/bin/sh
set -eu

node /usr/local/bin/write-chromium-flags.js

/wrapper &
wrapper_pid=$!

graceful_stop() {
  supervisorctl -c /etc/supervisor/supervisord.conf stop chromium >/dev/null 2>&1 || true
  kill -TERM "${wrapper_pid}" 2>/dev/null || true
  wait "${wrapper_pid}" 2>/dev/null || true
  exit 0
}

trap graceful_stop TERM INT
wait "${wrapper_pid}"
