#!/bin/sh
set -eu

# Addness CLI installer
# Usage: curl -fsSL https://cli.addness.co/install.sh | sh

CDN_BASE="${ADDNESS_CDN_BASE:-https://cli.addness.co}"
INSTALL_DIR="${ADDNESS_INSTALL_DIR:-/usr/local/bin}"
VERSION="${ADDNESS_VERSION:-latest}"

# Colors (disabled when not a TTY)
if [ -t 1 ]; then
  BOLD='\033[1m'
  GREEN='\033[1;32m'
  RED='\033[1;31m'
  DIM='\033[2m'
  RESET='\033[0m'
else
  BOLD=''
  GREEN=''
  RED=''
  DIM=''
  RESET=''
fi

info() {
  printf "  %b\n" "$1"
}

ok() {
  printf "  ${GREEN}ok${RESET} %b\n" "$1"
}

err() {
  printf "  ${RED}error${RESET} %s\n" "$1" >&2
}

main() {
  printf "\n"
  info "${BOLD}Addness CLI Installer${RESET}"
  printf "\n"

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
      err "Unsupported OS: ${OS}"
      exit 1
      ;;
  esac

  case "${ARCH}" in
    x86_64)  ARCH="x86_64" ;;
    aarch64|arm64) ARCH="aarch64" ;;
    *)
      err "Unsupported architecture: ${ARCH}"
      exit 1
      ;;
  esac

  if [ "${OS}" = "unknown-linux-gnu" ] && [ "${ARCH}" = "aarch64" ]; then
    err "Linux aarch64 is not yet supported"
    exit 1
  fi

  TARGET="${ARCH}-${OS}"
  info "Platform: ${BOLD}${TARGET}${RESET}"
}

download_and_install() {
  BASE_URL="${CDN_BASE}/releases/${VERSION}"
  ARCHIVE="addness-${TARGET}.tar.gz"
  URL="${BASE_URL}/${ARCHIVE}"
  SHA_URL="${URL}.sha256"

  TMPDIR="$(mktemp -d)"
  trap 'rm -rf "${TMPDIR}"' EXIT

  info "Downloading ${DIM}${URL}${RESET}"
  if [ -t 1 ]; then
    curl -fSL --progress-bar "${URL}" -o "${TMPDIR}/${ARCHIVE}"
  else
    curl -fsSL "${URL}" -o "${TMPDIR}/${ARCHIVE}"
  fi
  curl -fsSL "${SHA_URL}" -o "${TMPDIR}/${ARCHIVE}.sha256"

  info "Verifying checksum..."
  cd "${TMPDIR}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "${ARCHIVE}.sha256" >/dev/null
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "${ARCHIVE}.sha256" >/dev/null
  else
    err "No sha256 tool found. Cannot verify binary integrity."
    exit 1
  fi
  ok "Checksum verified"

  info "Installing to ${BOLD}${INSTALL_DIR}/addness${RESET}"
  tar -xzf "${ARCHIVE}"

  if [ -w "${INSTALL_DIR}" ]; then
    mv addness "${INSTALL_DIR}/addness"
  else
    sudo mv addness "${INSTALL_DIR}/addness"
  fi

  chmod +x "${INSTALL_DIR}/addness"
  ok "Installed"
}

verify_installation() {
  printf "\n"
  if command -v addness >/dev/null 2>&1; then
    INSTALLED_VERSION="$(addness --version 2>/dev/null || printf "unknown")"
    info "${GREEN}Addness CLI installed successfully!${RESET} ${DIM}(${INSTALLED_VERSION})${RESET}"
  else
    info "Installed to ${INSTALL_DIR}/addness"
    info "${DIM}Make sure ${INSTALL_DIR} is in your PATH${RESET}"
  fi

  printf "\n"
  info "Get started:"
  info "  ${BOLD}addness login${RESET}       Log in to your account"
  info "  ${BOLD}addness goals list${RESET}  View your goals"
  printf "\n"
}

main
