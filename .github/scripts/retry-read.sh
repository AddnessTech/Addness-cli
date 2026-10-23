#!/usr/bin/env bash
# Only wrap read operations: retrying writes can duplicate side effects.
retry_read() {
  local attempt status output
  output="$(mktemp)" || return 1
  for attempt in 1 2 3; do
    if "$@" >"$output"; then
      cat "$output"
      rm -f "$output"
      return 0
    else
      status=$?
    fi
    # git ls-remote --exit-code returns 2 for an absent ref, not a transport error.
    if [ "$status" -eq 2 ] && [ "${1:-}" = git ] && [ "${2:-}" = ls-remote ]; then
      rm -f "$output"
      return "$status"
    fi
    echo "Read attempt ${attempt}/3 failed (exit ${status}): $*" >&2
    if [ "$attempt" -lt 3 ]; then
      sleep "$((attempt * 2))"
    fi
  done
  rm -f "$output"
  echo "::error::Read failed after 3 attempts: $*" >&2
  return "$status"
}
