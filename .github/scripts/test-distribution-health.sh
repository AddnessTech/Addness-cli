#!/usr/bin/env bash
set -euo pipefail

script_directory="$(cd "$(dirname "$0")" && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
mkdir -p "$test_root/bin" "$test_root/public/releases/"{v0.16.0,latest}
printf 'v0.16.0\n' > "$test_root/public/releases/latest/version.txt"
printf '#!/bin/sh\necho install\n' > "$test_root/public/install.sh"
printf 'Write-Host install\n' > "$test_root/public/install.ps1"
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  extension='tar.gz'
  if [ "$target" = 'x86_64-pc-windows-msvc' ]; then extension='zip'; fi
  archive="addness-$target.$extension"
  printf 'release fixture for %s\n' "$target" > "$test_root/public/releases/v0.16.0/$archive"
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$test_root/public/releases/v0.16.0/$archive")"
  else
    digest="$(shasum -a 256 "$test_root/public/releases/v0.16.0/$archive")"
  fi
  printf '%s  %s\n' "${digest%% *}" "$archive" > "$test_root/public/releases/v0.16.0/$archive.sha256"
  cp "$test_root/public/releases/v0.16.0/$archive" "$test_root/public/releases/latest/"
  cp "$test_root/public/releases/v0.16.0/$archive.sha256" "$test_root/public/releases/latest/"
done

cat > "$test_root/bin/curl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
url=''
destination=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    -k|--insecure) echo 'TLS verification must not be disabled' >&2; exit 99 ;;
    --output) destination="$2"; shift ;;
    https://*) url="$1" ;;
  esac
  shift
done
printf '%s\n' "$url" >> "$TEST_ROOT/requests"
if [ "${TEST_FAILURE:-}" = 'tls' ]; then exit 60; fi
relative="${url#https://cli.addness.com/}"
case "${TEST_FAILURE:-}:$relative" in
  stale:releases/latest/version.txt) printf 'v0.15.1\n' > "$destination"; exit 0 ;;
  invalid-version:releases/latest/version.txt) printf '<html>broken</html>\n' > "$destination"; exit 0 ;;
  missing:releases/v0.16.0/addness-x86_64-pc-windows-msvc.zip) exit 22 ;;
  corrupt:releases/latest/addness-x86_64-pc-windows-msvc.zip) printf 'corrupt' > "$destination"; exit 0 ;;
  checksum-name:releases/latest/addness-aarch64-apple-darwin.tar.gz.sha256|checksum-name:releases/v0.16.0/addness-aarch64-apple-darwin.tar.gz.sha256)
    printf '%064d  ../../unexpected\n' 0 > "$destination"; exit 0 ;;
esac
cp "$TEST_ROOT/public/$relative" "$destination"
MOCK
cat > "$test_root/bin/openssl" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  s_client)
    [[ " $* " == *' -verify_hostname cli.addness.com '* ]]
    [[ " $* " == *' -verify_return_error '* ]]
    printf 'fixture certificate\n'
    ;;
  x509)
    [[ " $* " == *' -checkend 1209600 '* ]]
    [ "${TEST_FAILURE:-}" != 'expiring' ]
    ;;
  *) exit 99 ;;
esac
MOCK
cat > "$test_root/bin/sleep" <<'MOCK'
#!/usr/bin/env bash
exit 0
MOCK
chmod +x "$test_root/bin/"*

run_check() {
  env PATH="$test_root/bin:$PATH" TEST_ROOT="$test_root" TEST_FAILURE="${1:-}" \
    bash "$script_directory/check-distribution.sh" v0.16.0
}
run_check > "$test_root/success.log"
test "$(grep -c '^Verified ' "$test_root/success.log")" = 4
grep -q '^Distribution healthy: v0.16.0' "$test_root/success.log"
for failure in tls stale invalid-version missing corrupt checksum-name expiring; do
  : > "$test_root/requests"
  if run_check "$failure" > "$test_root/$failure.log" 2>&1; then
    echo "Unexpected success for $failure" >&2
    exit 1
  fi
  if [ "$failure" = 'stale' ]; then
    test "$(wc -l < "$test_root/requests" | tr -d ' ')" = 12
  fi
done
echo 'distribution-health: valid release and all failure scenarios passed'
