#!/usr/bin/env bash
set -euo pipefail

TEXT="${1:-📢 [알림] 작업 진행 중...}"
CHAT_ID="${COKACDIR_OWNER_USER_ID:-7556028243}"

# 현재 활성화된 Cokacdir 봇 토큰을 동적으로 탐색
BOT_TOKEN=""
if command -v docker >/dev/null 2>&1; then
    BOT_TOKEN=$(docker exec cokacdir env 2>/dev/null | grep '^COKACDIR_BOT_TOKEN=' | cut -d'=' -f2 || true)
fi

if [[ -z "${BOT_TOKEN}" ]]; then
    if [[ -f "/home/cokac/.cokacdir/bot-tokens" ]]; then
        BOT_TOKEN=$(head -n 1 /home/cokac/.cokacdir/bot-tokens | tr -d '\r\n')
    elif [[ -f "/home/linuxse/cokacdir/data/cokacdir/bot-tokens" ]]; then
        BOT_TOKEN=$(head -n 1 /home/linuxse/cokacdir/data/cokacdir/bot-tokens | tr -d '\r\n')
    fi
fi

if [[ -z "${BOT_TOKEN}" ]]; then
    echo "경고: 활성화된 Telegram 봇 토큰을 탐색하지 못했습니다." >&2
    exit 1
fi

curl -s -X POST "https://api.telegram.org/bot${BOT_TOKEN}/sendMessage" \
  -d "chat_id=${CHAT_ID}" \
  -d "text=${TEXT}" >/dev/null
