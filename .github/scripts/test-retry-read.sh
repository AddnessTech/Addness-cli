#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=.github/scripts/retry-read.sh
source "$(dirname "$0")/retry-read.sh"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
# Keep retries deterministic and immediate; record the requested delays instead of sleeping.
sleep() { echo "$1" >>"$test_root/sleeps"; }
flaky() {
  local count
  count="$(cat "$test_root/count")"
  echo "$((count + 1))" > "$test_root/count"
  if [ "$count" -lt 4 ]; then
    echo 'partial failed response'
    return 1
  fi
  echo '{"tag_name":"v1"}'
}
echo 0 > "$test_root/count"
: > "$test_root/sleeps"
actual="$(retry_read flaky 2>"$test_root/log")"
test "$actual" = '{"tag_name":"v1"}'
test "$(cat "$test_root/count")" = 5
test "$(tr '\n' ' ' < "$test_root/sleeps")" = '5 15 45 90 '
always_fails() { echo partial; return 7; }
status=0
: > "$test_root/sleeps"
retry_read always_fails >"$test_root/out" 2>"$test_root/log" || status=$?
test "$status" = 7
test ! -s "$test_root/out"
grep -q 'after 5 attempts' "$test_root/log"
test "$(wc -l < "$test_root/sleeps" | tr -d ' ')" = 4
git() { echo call >>"$test_root/calls"; return 2; }
status=0
retry_read git ls-remote --exit-code --heads origin absent >"$test_root/out" || status=$?
test "$status" = 2
test "$(wc -l < "$test_root/calls" | tr -d ' ')" = 1
echo 'retry-read: all checks passed'
