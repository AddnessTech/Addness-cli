#!/usr/bin/env bash
set -euo pipefail

cdn_base='https://cli.addness.com'
expected_version="${1:-}"
if [ -n "$expected_version" ] && ! [[ "$expected_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo 'Expected version must be a stable vX.Y.Z tag' >&2
  exit 1
fi

check_root="$(mktemp -d)"
trap 'rm -rf "$check_root"' EXIT

download() {
  curl --fail --silent --show-error --connect-timeout 10 --max-time 90 \
    --retry 2 --retry-delay 2 "$cdn_base/$1" --output "$2"
}

# CloudFront invalidation is asynchronous. Only a stale, otherwise valid version
# is retried here; TLS errors must remain visible and must never be bypassed.
for attempt in {1..12}; do
  download 'releases/latest/version.txt' "$check_root/version.txt"
  actual_version="$(tr -d '\r\n' < "$check_root/version.txt")"
  if ! [[ "$actual_version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Invalid published version: $actual_version" >&2
    exit 1
  fi
  if [ -z "$expected_version" ] || [ "$actual_version" = "$expected_version" ]; then
    break
  fi
  if [ "$attempt" -eq 12 ]; then
    echo "Expected $expected_version, found $actual_version after CDN propagation wait" >&2
    exit 1
  fi
  sleep 15
done

openssl s_client -connect cli.addness.com:443 -servername cli.addness.com \
  -verify_hostname cli.addness.com -verify_return_error </dev/null \
  > "$check_root/certificate.pem" 2> "$check_root/tls.log" || {
    cat "$check_root/tls.log" >&2
    exit 1
  }
openssl x509 -in "$check_root/certificate.pem" -noout -checkend 1209600 || {
  echo 'Distribution TLS certificate expires within 14 days' >&2
  exit 1
}

download 'install.sh' "$check_root/install.sh"
download 'install.ps1' "$check_root/install.ps1"
test -s "$check_root/install.sh"
test -s "$check_root/install.ps1"
bash -n "$check_root/install.sh"

for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
  extension='tar.gz'
  if [ "$target" = 'x86_64-pc-windows-msvc' ]; then extension='zip'; fi
  archive="addness-$target.$extension"
  download "releases/$actual_version/$archive.sha256" "$check_root/versioned.sha256"
  download "releases/latest/$archive.sha256" "$check_root/latest.sha256"
  cmp "$check_root/versioned.sha256" "$check_root/latest.sha256"
  read -r expected_hash checksum_name < "$check_root/versioned.sha256"
  checksum_name="${checksum_name%$'\r'}"
  if ! [[ "$expected_hash" =~ ^[a-fA-F0-9]{64}$ ]] || [ "$checksum_name" != "$archive" ]; then
    echo "Invalid checksum record for $archive" >&2
    exit 1
  fi
  for version in "$actual_version" latest; do
    download "releases/$version/$archive" "$check_root/$archive"
    if command -v sha256sum >/dev/null 2>&1; then
      actual_hash="$(sha256sum "$check_root/$archive")"
    else
      actual_hash="$(shasum -a 256 "$check_root/$archive")"
    fi
    if [ "${actual_hash%% *}" != "$expected_hash" ]; then
      echo "Checksum mismatch: $version/$archive" >&2
      exit 1
    fi
  done
  echo "Verified $actual_version: $archive (versioned and latest)"
done
echo "Distribution healthy: $actual_version; TLS valid for at least 14 days"
