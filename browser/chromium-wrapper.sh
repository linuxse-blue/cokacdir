#!/bin/sh
set -u

/usr/local/bin/chromium-launcher &
launcher_pid=$!

graceful_stop() {
  if node /usr/local/bin/browser-graceful-stop.js; then
    wait "${launcher_pid}" 2>/dev/null || true
    runuser -u kernel -- node /usr/local/bin/mark-profile-clean.js
  else
    kill -TERM "${launcher_pid}" 2>/dev/null || true
    wait "${launcher_pid}" 2>/dev/null || true
  fi
  exit 0
}

trap graceful_stop TERM INT
wait "${launcher_pid}"
