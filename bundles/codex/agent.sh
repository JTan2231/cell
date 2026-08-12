#!/bin/sh
set -eu

bundle_dir=$(CDPATH= cd "$(dirname "$0")" && pwd)

exec codex exec \
  --ephemeral \
  --ignore-user-config \
  --ignore-rules \
  --disable shell_tool \
  --disable unified_exec \
  --skip-git-repo-check \
  --sandbox read-only \
  --color never \
  --model gpt-5.6-terra \
  -c 'model_reasoning_effort="medium"' \
  --output-schema "$bundle_dir/generated-tree.schema.json" \
  -
