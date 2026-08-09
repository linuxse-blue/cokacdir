#!/bin/sh
set -eu

display_num="${DISPLAY_NUM:-1}"
display=":${display_num}"

while [ ! -S "/tmp/.X11-unix/X${display_num}" ]; do
  sleep 0.2
done

if [ -n "${NOVNC_PASSWORD:-}" ]; then
  umask 077
  x11vnc -storepasswd "${NOVNC_PASSWORD}" /tmp/x11vnc.pass >/dev/null
  unset NOVNC_PASSWORD
  set -- -rfbauth /tmp/x11vnc.pass
else
  set -- -nopw
fi

exec x11vnc \
  -display "${display}" \
  -forever \
  -shared \
  -localhost \
  -rfbport 5900 \
  -xkb \
  -repeat \
  -quiet \
  "$@"
