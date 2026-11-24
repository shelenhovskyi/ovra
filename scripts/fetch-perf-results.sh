#!/usr/bin/env bash
# Downloads the speed gate results (`perf-results` artifacts) of recent CI runs
# on main into <out>/<run id>/ for the site's Speed page. Needs `gh` with a
# token that can read Actions artifacts (GH_TOKEN). A failed download only
# drops that run: the site still builds.
# Usage: fetch-perf-results.sh <out> [count]
set -uo pipefail
out=${1:?usage: fetch-perf-results.sh <out> [count]}
count=${2:-20}
repo=${GITHUB_REPOSITORY:-shelenhovskyi/ovra}
mkdir -p "$out"

if ! list=$(gh api "repos/$repo/actions/artifacts?name=perf-results&per_page=100" \
  --jq "[.artifacts[] | select(.expired | not) | select(.workflow_run.head_branch == \"main\")][:$count][]
        | [.id, .workflow_run.id, .workflow_run.head_sha, .created_at] | @tsv"); then
  echo "warning: cannot list perf-results artifacts; the Speed page will show targets only" >&2
  exit 0
fi

while IFS=$'\t' read -r id run sha at; do
  [ -n "$id" ] || continue
  dir="$out/$run"
  mkdir -p "$dir"
  if ! gh api "repos/$repo/actions/artifacts/$id/zip" > "$dir/results.zip" ||
    ! unzip -q -o "$dir/results.zip" metrics.jsonl -d "$dir"; then
    echo "warning: skipping run $run: download failed" >&2
    rm -rf "$dir"
    continue
  fi
  rm "$dir/results.zip"
  printf '{"run":%s,"sha":"%s","at":"%s","url":"https://github.com/%s/actions/runs/%s"}\n' \
    "$run" "$sha" "$at" "$repo" "$run" > "$dir/meta.json"
done <<< "$list"
echo "speed gate results: $(find "$out" -name metrics.jsonl | wc -l) run(s)"
