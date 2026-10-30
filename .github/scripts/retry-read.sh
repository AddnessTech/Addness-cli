#!/usr/bin/env bash
# Only wrap read operations: retrying writes can duplicate side effects.
# Backs off 5s/15s/45s/90s across 5 attempts: GitHub API stream resets
# ("stream error: ... CANCEL; received from peer") keep recurring for a
# while, so the previous 2s/4s waits over 3 attempts were not enough.
retry_read() {
  local attempt status output max_attempts
  local -a delays=(5 15 45 90)
  max_attempts=$(( ${#delays[@]} + 1 ))
  output="$(mktemp)" || return 1
  for (( attempt = 1; attempt <= max_attempts; attempt++ )); do
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
    echo "Read attempt ${attempt}/${max_attempts} failed (exit ${status}): $*" >&2
    if [ "$attempt" -lt "$max_attempts" ]; then
      sleep "${delays[attempt - 1]}"
    fi
  done
  rm -f "$output"
  echo "::error::Read failed after ${max_attempts} attempts: $*" >&2
  return "$status"
}
