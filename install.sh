#!/bin/sh
set -eu

# Addness CLI installer
# Usage: curl -fsSL https://cli.addness.com/install.sh | sh

CDN_BASE="${ADDNESS_CDN_BASE:-https://cli.addness.com}"
INSTALL_DIR="${ADDNESS_INSTALL_DIR:-/usr/local/bin}"
VERSION="${ADDNESS_VERSION:-latest}"

# Colors (disabled when not a TTY)
if [ -t 1 ]; then
  BOLD='\033[1m'
  GREEN='\033[1;32m'
  BLUE='\033[1;34m'
  RED='\033[1;31m'
  DIM='\033[2m'
  RESET='\033[0m'
else
  BOLD=''
  GREEN=''
  BLUE=''
  RED=''
  DIM=''
  RESET=''
fi

banner() {
  printf "\n"
  for line in \
    "  ${BLUE}                                        ." \
    "  ${BLUE}                   .:=+*###***+=:.    =:" \
    "  ${BLUE}               .=*%@@%*=:.    .:=**+#=" \
    "  ${BLUE}            .:*@@@@*:.            :#%*:" \
    "  ${BLUE}          .+@@@@@*.            :+%%=. .+=" \
    "  ${BLUE}         =@@@@@@:          .=*%%+.     ::" \
    "  ${BLUE}       .*@@@@@@.      .:+*%%%#=.        :" \
    "  ${BLUE}      .@@@@@@@:  =+*#%%%%%%+:" \
    "  ${BLUE}     .@@@@@@@+ .*%%%%%%#+:" \
    "  ${BLUE}    .@@@@@@@@. *%%%%*=." \
    "  ${BLUE}    *@@@@@@@+ .%%*=." \
    "  ${BLUE}   :@@@@@@@@." \
    "  ${BLUE}   #@@@@@@@*" \
    "  ${BLUE}   ++==::..${RESET}"
  do
    printf "%b\n" "$line"
    sleep 0.03
  done
  printf "\n"
  for line in \
    "  ${BOLD} _         _            _       _     _                       _ _   _ " \
    "  | |    ___| |_ ___     / \\   __| | __| |_ __   ___  ___ ___  (_) |_| |" \
    "  | |   / _ \\ __/ __|   / _ \\ / _\` |/ _\` | '_ \\ / _ \\/ __/ __| | | __| |" \
    "  | |__|  __/ |_\\__ \\  / ___ \\ (_| | (_| | | | |  __/\\__ \\__ \\ | | |_|_|" \
    "  |_____\\___|\\__|___/ /_/   \\_\\__,_|\\__,_|_| |_|\\___||___/___/ |_|\\__(_)${RESET}"
  do
    printf "%b\n" "$line"
    sleep 0.03
  done
  printf "\n"
}

info() {
  printf "  ${DIM}>${RESET} %b\n" "$1"
}

step() {
  printf "  ${DIM}>${RESET} %b..." "$1"
}

step_ok() {
  printf " ${GREEN}done${RESET}\n"
}

ok() {
  printf "  ${GREEN}*${RESET} %b\n" "$1"
}

err() {
  printf "  ${RED}!${RESET} %s\n" "$1" >&2
}

main() {
  banner
  detect_platform
  printf "\n"
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
  info "Platform  ${BOLD}${TARGET}${RESET}"
  info "Version   ${BOLD}${VERSION}${RESET}"
}

download_and_install() {
  BASE_URL="${CDN_BASE}/releases/${VERSION}"
  ARCHIVE="addness-${TARGET}.tar.gz"
  URL="${BASE_URL}/${ARCHIVE}"
  SHA_URL="${URL}.sha256"

  TMPDIR="$(mktemp -d)"
  trap 'rm -rf "${TMPDIR}"' EXIT

  # ダウンロード（プログレスバー付き）
  if [ -t 1 ]; then
    printf "  ${DIM}>${RESET} Downloading\n"
    curl -fSL --progress-bar "${URL}" -o "${TMPDIR}/${ARCHIVE}"
    printf "\033[1A\033[2K"
    printf "  ${DIM}>${RESET} Downloading     ${GREEN}100%%${RESET}\n"
  else
    printf "  > Downloading..."
    curl -fsSL "${URL}" -o "${TMPDIR}/${ARCHIVE}"
    printf " done\n"
  fi
  curl -fsSL "${SHA_URL}" -o "${TMPDIR}/${ARCHIVE}.sha256"

  step "Verifying checksum"
  cd "${TMPDIR}"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "${ARCHIVE}.sha256" >/dev/null
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "${ARCHIVE}.sha256" >/dev/null
  else
    printf " ${RED}failed${RESET}\n"
    err "No sha256 tool found. Cannot verify binary integrity."
    exit 1
  fi
  step_ok

  step "Installing to ${INSTALL_DIR}"
  tar -xzf "${ARCHIVE}"

  mkdir -p "${INSTALL_DIR}"
  mv addness "${INSTALL_DIR}/addness"
  chmod +x "${INSTALL_DIR}/addness"
  step_ok
}

verify_installation() {
  printf "\n"
  if command -v addness >/dev/null 2>&1; then
    ok "${GREEN}Addness CLI installed successfully!${RESET}"
  else
    ok "Installed to ${BOLD}${INSTALL_DIR}/addness${RESET}"
    printf "\n"
    printf "  ${DIM}Add to your PATH:${RESET}\n"
    SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"
    case "${SHELL_NAME}" in
      zsh)  RC_FILE="\$HOME/.zshrc" ;;
      bash) RC_FILE="\$HOME/.bashrc" ;;
      fish) RC_FILE="\$HOME/.config/fish/config.fish" ;;
      *)    RC_FILE="your shell config" ;;
    esac
    if [ "${SHELL_NAME}" = "fish" ]; then
      printf "  ${BOLD}  fish_add_path %s${RESET}\n" "${INSTALL_DIR}"
    else
      printf "  ${BOLD}  echo 'export PATH=\"%s:\$PATH\"' >> %s${RESET}\n" "${INSTALL_DIR}" "${RC_FILE}"
    fi
    printf "  ${DIM}Then restart your terminal or run: source %s${RESET}\n" "${RC_FILE}"
  fi

  printf "\n"
  printf "  ${DIM}Get started:${RESET}\n"
  printf "  ${BOLD}  addness login${RESET}      ${DIM}Log in to your account${RESET}\n"
  printf "  ${BOLD}  addness goal list${RESET}  ${DIM}View your goals${RESET}\n"
  printf "\n"
  printf "  ${DIM}AI integration:${RESET}\n"
  printf "  ${BOLD}  addness skills${RESET}     ${DIM}Output AI skills prompt${RESET}\n"
  printf "  ${BOLD}  addness skills >> CLAUDE.md${RESET}  ${DIM}Add to your project${RESET}\n"
  printf "\n"
}

main
