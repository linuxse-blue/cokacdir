---
name: herdr
description: Herdr pane multiplexer management skill for agent background worker management. Reuse dedicated agy-worker pane for background processes.
---

# Herdr Worker Management Skill

This skill provides guidelines and scripts for managing background processes in Herdr.

## Background Worker Rule (`agy-worker`)

- Never split new Herdr panes repeatedly for background tasks.
- Always use the dedicated `agy-worker` pane.
- If `agy-worker` pane exists, reuse it.
- If `agy-worker` pane does not exist, split a new pane once and rename it to `agy-worker`.

## Helper Script

To get the existing `agy-worker` pane ID or create it if missing:

```bash
.agents/skills/herdr/scripts/ensure_worker_pane.sh [CWD]
```

Example usage in commands:

```bash
WORKER_PANE_ID=$(.agents/skills/herdr/scripts/ensure_worker_pane.sh /home/linuxse/cokacdir)
herdr pane send-text "$WORKER_PANE_ID" "docker compose build cokacdir"
herdr pane send-keys "$WORKER_PANE_ID" enter
```
