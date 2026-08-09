---
name: telegram-push
description: Send real-time progress push notifications to Telegram bot (@cj_li_bot) during multi-step tasks.
---

# Telegram Progress Push Skill

Use this skill to send real-time Push notification updates to the user's Telegram chat (`@cj_li_bot`) during long-running or multi-step operations.

## Usage

Run the helper script directly from bash:

```bash
.agents/skills/telegram-push/scripts/push.sh "📢 [진행중 1/3] 소스 코드 분석 완료"
```

## When to use

- Long-running multi-step refactoring tasks
- Docker build and compilation checkpoints
- Intermediate status updates before returning the final response
