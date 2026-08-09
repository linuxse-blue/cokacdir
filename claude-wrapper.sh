#!/usr/bin/env bash
set -euo pipefail

CLAUDE_RUN_UID="${CLAUDE_RUN_UID:-1000}"
CLAUDE_RUN_GID="${CLAUDE_RUN_GID:-1000}"
CLAUDE_HOME="${CLAUDE_HOME:-/home/cokac}"
CLAUDE_BIN="${CLAUDE_BIN:-/home/cokac/.agents/npm/bin/claude}"

export HOME="${CLAUDE_HOME}"

if [ "$(id -u)" = "0" ]; then
  mkdir -p "${CLAUDE_HOME}" "${CLAUDE_HOME}/.claude" /workspace
  chown -R "${CLAUDE_RUN_UID}:${CLAUDE_RUN_GID}" "${CLAUDE_HOME}" 2>/dev/null || true
  exec setpriv --reuid "${CLAUDE_RUN_UID}" --regid "${CLAUDE_RUN_GID}" --clear-groups "${CLAUDE_BIN}" "$@"
fi

exec "${CLAUDE_BIN}" "$@"
