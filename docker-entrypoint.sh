#!/usr/bin/env bash
set -euo pipefail

export AGENT_HOME="${AGENT_HOME:-/home/cokac/.agents}"
export NPM_CONFIG_PREFIX="${NPM_CONFIG_PREFIX:-${AGENT_HOME}/npm}"
export NODE_PATH="${NODE_PATH:-${NPM_CONFIG_PREFIX}/lib/node_modules}"
export PATH="${NPM_CONFIG_PREFIX}/bin:${AGENT_HOME}/bin:${PATH}"
export COKAC_CODEX_PATH="${COKAC_CODEX_PATH:-${NPM_CONFIG_PREFIX}/bin/codex}"
export COKAC_CLAUDE_PATH="${COKAC_CLAUDE_PATH:-/usr/local/bin/claude}"
export COKAC_AGY_PATH="${COKAC_AGY_PATH:-/usr/local/bin/agy}"
export COKAC_HERDR_PATH="${COKAC_HERDR_PATH:-/usr/local/bin/herdr}"

agent_uid="${CLAUDE_RUN_UID:-1000}"
agent_gid="${CLAUDE_RUN_GID:-1000}"
npm_bin_dir="${NPM_CONFIG_PREFIX}/bin"
agy_bin="${AGENT_HOME}/bin/agy"
herdr_mode="${HERDR_MODE:-external}"

run_as_agent() {
  setpriv --reuid "${agent_uid}" --regid "${agent_gid}" --clear-groups \
    env HOME=/home/cokac "$@"
}

mkdir -p /workspace "${npm_bin_dir}" "${AGENT_HOME}/bin" /home/cokac/.cokacdir /home/cokac/.codex /home/cokac/.claude /home/cokac/.local/share
chown -R "${CLAUDE_RUN_UID:-1000}:${CLAUDE_RUN_GID:-1000}" /home/cokac 2>/dev/null || true

install_npm_agent() {
  local binary="$1"
  local package="$2"

  if [ -x "${npm_bin_dir}/${binary}" ]; then
    echo "Using installed ${binary}: ${npm_bin_dir}/${binary}"
    return
  fi

  echo "Installing ${package} into ${NPM_CONFIG_PREFIX}..."
  run_as_agent env NPM_CONFIG_PREFIX="${NPM_CONFIG_PREFIX}" \
    npm install --global "${package}"
}

install_npm_agent codex @openai/codex
install_npm_agent claude @anthropic-ai/claude-code
install_npm_agent playwright-core playwright-core@1.61.1

if [ -x "${agy_bin}" ]; then
  echo "Using installed agy: ${agy_bin}"
else
  echo "Installing agy into ${AGENT_HOME}/bin..."
  curl -fsSL https://antigravity.google/cli/install.sh | \
    run_as_agent bash -s -- --dir "${AGENT_HOME}/bin"
fi

cd /workspace

echo "Starting cokacdir..."
echo "COKAC_CODEX_PATH=${COKAC_CODEX_PATH}"
echo "COKAC_CLAUDE_PATH=${COKAC_CLAUDE_PATH}"
echo "COKAC_AGY_PATH=${COKAC_AGY_PATH}"
echo "COKAC_HERDR_PATH=${COKAC_HERDR_PATH}"
echo "HERDR_MODE=${herdr_mode}"

start_internal_herdr() {
  local workdir="${COKAC_HERDR_WORKDIR:-/workspace}"
  local agent_name="${COKAC_HERDR_AGENT:-worker}"
  local agent_kind="${COKAC_HERDR_AGENT_KIND:-codex}"
  local start_timeout_ms="${COKAC_HERDR_START_TIMEOUT_MS:-60000}"
  local server_log="/tmp/herdr-server.log"
  local pane_json pane_id

  if [ ! -d "${workdir}" ]; then
    echo "Internal Herdr workdir does not exist: ${workdir}" >&2
    return 1
  fi

  echo "Starting internal Herdr server..."
  run_as_agent herdr server >"${server_log}" 2>&1 &
  internal_herdr_pid=$!

  local attempt=0
  until run_as_agent herdr status server 2>/dev/null | grep -q "status: running"; do
    attempt=$((attempt + 1))
    if ! kill -0 "${internal_herdr_pid}" 2>/dev/null || [ "${attempt}" -ge 100 ]; then
      echo "Internal Herdr server failed to become ready." >&2
      sed -n '1,120p' "${server_log}" >&2 || true
      return 1
    fi
    sleep 0.1
  done

  if run_as_agent herdr agent get "${agent_name}" >/dev/null 2>&1; then
    echo "Using internal Herdr agent: ${agent_name}"
    return 0
  fi

  pane_json="$(run_as_agent herdr pane list)"
  pane_id="$(printf '%s' "${pane_json}" | python3 -c '
import json, os, sys
workdir = os.environ.get("COKAC_HERDR_WORKDIR", "/workspace")
for pane in json.load(sys.stdin).get("result", {}).get("panes", []):
    if pane.get("cwd") == workdir and not pane.get("agent"):
        print(pane["pane_id"])
        break
')"

  if [ -z "${pane_id}" ]; then
    pane_json="$(run_as_agent herdr workspace create \
      --cwd "${workdir}" --label sandbox --no-focus)"
    pane_id="$(printf '%s' "${pane_json}" | python3 -c '
import json, sys
print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])
')"
  fi

  echo "Starting internal Herdr agent ${agent_name} (${agent_kind}) in ${pane_id}..."
  run_as_agent herdr agent start "${agent_name}" \
    --kind "${agent_kind}" --pane "${pane_id}" --timeout "${start_timeout_ms}"
}

stop_internal_processes() {
  trap - TERM INT
  if [ -n "${internal_cokacdir_pid:-}" ]; then
    kill -TERM "${internal_cokacdir_pid}" 2>/dev/null || true
  fi
  run_as_agent herdr server stop >/dev/null 2>&1 || true
  if [ -n "${internal_herdr_pid:-}" ]; then
    kill -TERM "${internal_herdr_pid}" 2>/dev/null || true
  fi
}

persistent_token_file="${COKACDIR_BOT_TOKEN_FILE:-/home/cokac/.cokacdir/bot-tokens}"
runtime_token_file="/tmp/cokacdir-bot-tokens"
deduplicated_token_file="${runtime_token_file}.deduplicated"

umask 077
: > "${runtime_token_file}"

if [ -n "${COKACDIR_BOT_TOKEN:-}" ]; then
  printf '%s\n' "${COKACDIR_BOT_TOKEN}" >> "${runtime_token_file}"
fi

if [ -r "${persistent_token_file}" ]; then
  while IFS= read -r token || [ -n "${token}" ]; do
    token="${token%$'\r'}"
    if [ -n "${token}" ]; then
      printf '%s\n' "${token}" >> "${runtime_token_file}"
    fi
  done < "${persistent_token_file}"
fi

awk '!seen[$0]++' "${runtime_token_file}" > "${deduplicated_token_file}"
mv "${deduplicated_token_file}" "${runtime_token_file}"
token_count="$(wc -l < "${runtime_token_file}")"

case "${herdr_mode}" in
  external)
    if [ "${token_count}" -eq 0 ]; then
      echo "No bot token configured; starting cokacdir without the bot server."
      unset COKACDIR_BOT_TOKEN
      exec cokacdir
    fi

    echo "Starting ${token_count} bot(s) from a protected token file."
    unset COKACDIR_BOT_TOKEN
    exec cokacdir --ccserver-token-file "${runtime_token_file}"
    ;;
  internal)
    start_internal_herdr
    chown "${agent_uid}:${agent_gid}" "${runtime_token_file}"
    unset COKACDIR_BOT_TOKEN

    if [ "${token_count}" -eq 0 ]; then
      echo "No bot token configured; starting cokacdir without the bot server."
      run_as_agent cokacdir &
    else
      echo "Starting ${token_count} bot(s) from a protected token file."
      run_as_agent cokacdir --ccserver-token-file "${runtime_token_file}" &
    fi
    internal_cokacdir_pid=$!

    trap stop_internal_processes TERM INT
    set +e
    wait -n "${internal_herdr_pid}" "${internal_cokacdir_pid}"
    status=$?
    set -e
    stop_internal_processes
    wait "${internal_herdr_pid}" 2>/dev/null || true
    wait "${internal_cokacdir_pid}" 2>/dev/null || true
    exit "${status}"
    ;;
  *)
    echo "Invalid HERDR_MODE: ${herdr_mode} (expected external or internal)" >&2
    exit 1
    ;;
esac
