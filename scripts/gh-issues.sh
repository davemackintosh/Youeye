#!/usr/bin/env bash
# Dump GitHub issues — title + state + body + all comments — in a single
# pass, for quick triage inside an assistant context (Claude reads this
# output to plan work). One command keeps the allow-list small.
#
# Usage:
#   scripts/gh-issues.sh               # open issues with bodies + comments
#   scripts/gh-issues.sh --all         # open + closed
#   scripts/gh-issues.sh --closed      # closed only
#   scripts/gh-issues.sh 14 22 23      # specific numbers (any state)
#
# Requires: `gh` authenticated for the current repo.

set -euo pipefail

state="open"
nums=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --all) state="all"; shift ;;
        --closed) state="closed"; shift ;;
        --open) state="open"; shift ;;
        -h|--help)
            sed -n '3,12p' "$0" | sed 's|^# \{0,1\}||'
            exit 0
            ;;
        [0-9]*) nums+=("$1"); shift ;;
        *) echo "unknown arg: $1" >&2; exit 2 ;;
    esac
done

if [[ ${#nums[@]} -eq 0 ]]; then
    while IFS= read -r line; do
        nums+=("$line")
    done < <(gh issue list --state "$state" --limit 100 --json number --jq '.[].number')
fi

if [[ ${#nums[@]} -eq 0 ]]; then
    echo "(no issues)"
    exit 0
fi

for n in "${nums[@]}"; do
    echo "===== #$n ====="
    gh issue view "$n" --json title,state,body,comments --template \
'{{.title}}  [{{.state}}]

{{if .body}}{{.body}}
{{end}}{{range .comments}}
--- comment by {{.author.login}}
{{.body}}
{{end}}'
    echo
done
