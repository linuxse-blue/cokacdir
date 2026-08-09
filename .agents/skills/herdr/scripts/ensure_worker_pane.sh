#!/usr/bin/env bash
set -euo pipefail

WORKER_NAME="agy-worker"
CWD="${1:-/home/linuxse/cokacdir}"

# Check if agy-worker pane exists
PANES_JSON=$(herdr pane list)
EXISTING_PANE=$(jq -r --arg name "$WORKER_NAME" '
  .result.panes[]? | select(.label == $name or .label == "agy_worker") | .pane_id
' <<<"$PANES_JSON" | head -n 1)

if [[ -n "$EXISTING_PANE" ]]; then
  echo "$EXISTING_PANE"
  exit 0
fi

# Split new pane and rename to agy-worker
NEW_PANE_JSON=$(herdr pane split --direction right --cwd "$CWD")
NEW_PANE_ID=$(jq -r '.result.pane.pane_id' <<<"$NEW_PANE_JSON")

if [[ -z "$NEW_PANE_ID" || "$NEW_PANE_ID" == "null" ]]; then
  echo "Error: Failed to split pane" >&2
  exit 1
fi

herdr pane rename "$NEW_PANE_ID" "$WORKER_NAME" >/dev/null
echo "$NEW_PANE_ID"
