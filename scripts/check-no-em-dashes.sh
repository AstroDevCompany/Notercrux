#!/usr/bin/env bash
# Fails if the em dash character appears in tracked or new project files.
# The blueprint in Implementation/ is excluded: it is authored separately.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
hits="$(grep -rIn $'\xe2\x80\x94' . \
  --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.git --exclude-dir=dist \
  --exclude-dir=Implementation --exclude=pnpm-lock.yaml --exclude=Cargo.lock \
  --exclude=check-no-em-dashes.sh || true)"
if [ -n "$hits" ]; then
  echo "Em dashes are not allowed. Rephrase these (colon, period, or parentheses):" >&2
  echo "$hits" >&2
  exit 1
fi
echo "no em dashes"
