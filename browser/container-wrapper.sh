#!/bin/sh
set -eu

node /usr/local/bin/write-chromium-flags.js

# Ensure X11 display resolution matches BROWSER_WIDTH / BROWSER_HEIGHT from .env
(
  width="${WIDTH:-1920}"
  height="${HEIGHT:-1080}"
  display_num="${DISPLAY_NUM:-1}"
  for _ in $(seq 1 30); do
    if [ -S "/tmp/.X11-unix/X${display_num}" ]; then
      sleep 0.5
      DISPLAY=":${display_num}" xrandr -s "${width}x${height}" 2>/dev/null || true
      break
    fi
    sleep 0.2
  done
) &

/wrapper &
wrapper_pid=$!
watchdog_pid=""

if [ "${BROWSER_RENDERER_WATCHDOG:-1}" = "1" ]; then
  node /usr/local/bin/browser-renderer-watchdog.js &
  watchdog_pid=$!
fi

graceful_stop() {
  if [ -n "${watchdog_pid}" ]; then
    kill -TERM "${watchdog_pid}" 2>/dev/null || true
    wait "${watchdog_pid}" 2>/dev/null || true
  fi
  supervisorctl -c /etc/supervisor/supervisord.conf stop chromium >/dev/null 2>&1 || true
  kill -TERM "${wrapper_pid}" 2>/dev/null || true
  wait "${wrapper_pid}" 2>/dev/null || true
  exit 0
}

trap graceful_stop TERM INT
wait "${wrapper_pid}"
