#!/bin/sh
set -eu

# Addness CLI installer
# Usage: curl -fsSL https://cli.addness.co/install.sh | sh

CDN_BASE="${ADDNESS_CDN_BASE:-https://cli.addness.co}"
INSTALL_DIR="${ADDNESS_INSTALL_DIR:-/usr/local/bin}"
VERSION="${ADDNESS_VERSION:-latest}"

main() {
  detect_platform
  download_and_install
  verify_installation
}

detect_platform() {
  OS="$(uname -s)"
  ARCH="$(uname -m)"

  case "${OS}" in
    Darwin) OS="apple-darwin" ;;
    Linux)  OS="unknown-linux-gnu" ;;
    *)
      echo "Error: unsupported OS: ${OS}" >&2
      exit 1
      ;;
  esac

  case "${ARCH}" in
    x86_64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)
      echo "Error: unsupported architecture: ${ARCH}" >&2
      exit 1
      ;;
  esac

  # Linux aarch64 は未サポート
  if [ "${OS}" = "unknown-linux-gnu" ] && [ "${ARCH}" = "aarch64" ]; then
    echo "Error: Linux aarch64 is not yet supported" >&2
    exit 1
  fi

  TARGET="${ARCH}-${OS}"
  echo "Detected platform: ${TARGET}"
}

download_and_install() {
  if [ "${VERSION}" = "latest" ]; then
    BASE_URL="${CDN_BASE}/releases/latest"
  else
    BASE_URL="${CDN_BASE}/releases/${VERSION}"
  fi

  ARCHIVE="addness-${VERSION}-${TARGET}.tar.gz"
  URL="${BASE_URL}/${ARCHIVE}"
  SHA_URL="${URL}.sha256"

  TMPDIR="$(mktemp -d)"
  trap 'rm -rf "${TMPDIR}"' EXIT

  echo "Downloading ${URL}..."
  curl -fsSL "${URL}" -o "${TMPDIR}/${ARCHIVE}"
  curl -fsSL "${SHA_URL}" -o "${TMPDIR}/${ARCHIVE}.sha256"

  echo "Verifying checksum..."
  cd "${TMPDIR}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "${ARCHIVE}.sha256"
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "${ARCHIVE}.sha256"
  else
    echo "Warning: no sha256 tool found, skipping checksum verification" >&2
  fi

  echo "Installing to ${INSTALL_DIR}/addness..."
  tar -xzf "${ARCHIVE}"

  if [ -w "${INSTALL_DIR}" ]; then
    mv addness "${INSTALL_DIR}/addness"
  else
    sudo mv addness "${INSTALL_DIR}/addness"
  fi

  chmod +x "${INSTALL_DIR}/addness"
}

verify_installation() {
  if command -v addness >/dev/null 2>&1; then
    echo ""
    echo "Addness CLI installed successfully!"
    addness --version 2>/dev/null || true
  else
    echo ""
    echo "Installed to ${INSTALL_DIR}/addness"
    echo "Make sure ${INSTALL_DIR} is in your PATH"
  fi
}

main
