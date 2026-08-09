#!/usr/bin/env bash
set -euo pipefail

AGY_BIN="${AGY_BIN:-/home/cokac/.agents/bin/agy}"
args=("$@")

for ((i = 0; i < ${#args[@]}; i++)); do
    if [[ "${args[$i]}" == "--print" || "${args[$i]}" == "-p" || "${args[$i]}" == "--prompt" ]]; then
        next=$((i + 1))
        if ((next < ${#args[@]})) && [[ -z "${args[$next]}" ]]; then
            args[$next]=$(cat)
        fi
        break
    fi
done

exec "$AGY_BIN" "${args[@]}"
