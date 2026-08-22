# Repository Guidelines

## Project Structure & Module Organization

This repository is currently a minimal scaffold. Keep future files organized by purpose:

- `src/`: application code.
- `tests/`: automated tests mirroring `src/`.
- `docs/`: architecture, decisions, contributor notes, and durable memory.
- `assets/` or `public/`: static files when needed.

Do not treat `.agents/`, `.codex/`, or `.git/` as app directories.

## Build, Test, and Development Commands

No build system or test runner is present yet. When adding one, document exact root-level commands:

- `make test` or `npm test`: run tests.
- `make lint` or `npm run lint`: run formatting/static checks.
- `make dev` or `npm run dev`: start development.

## Coding Style & Naming Conventions

Use the formatter and linter introduced by the project stack. Until tooling exists, keep modules small, names descriptive, and changes focused. Use lowercase hyphenated Markdown names such as `project-overview.md`.

## Testing Guidelines

Add tests with new behavior or bug fixes. If tests are impractical, record manual verification in the PR or completion note. Verify with commands, logs, API responses, or browser checks.

## Agent Operating Rules

Think before coding: state assumptions when they affect the solution, surface meaningful tradeoffs, and ask only when missing information could cause wrong behavior, data loss, security issues, or major rework.

Prefer simplicity: implement the minimum needed, avoid speculative features, and do not create single-use abstractions. Make surgical changes; do not refactor adjacent code or remove pre-existing dead code unless asked.

For non-trivial work, define verifiable success criteria and consider relevant Backend, Frontend, QA, and Design perspectives. Use actual sub-agents only when they improve quality or parallel progress.

Use Korean for user-facing communication by default. Follow existing project language for code comments.

For multi-step or long-running tasks (e.g. builds, refactoring), send real-time progress Push notifications to the active Telegram bot using `.agents/skills/telegram-push/scripts/push.sh "📢 [진행중 ...]"` at each completed step.

## Knowledge Memory & Visual Maps

Use Markdown as the source of truth for durable project knowledge. Store long-term memory under `docs/memory/` when it exists or when asked. Prefer Mermaid diagrams in Markdown. Use draw.io only for presentation-grade diagrams or explicit requests. Do not store secrets, credentials, or temporary debugging output.

## Commit & Pull Request Guidelines

Use Korean Conventional Commits by default, for example `fix: DataTables 컬럼 불일치 오류 수정`.

Write commit messages only from the staged diff, keep the summary under 72 characters, and avoid inferred context. PRs should include a summary, verification evidence, linked issues, screenshots for UI changes, and any memory or visual-map updates.
