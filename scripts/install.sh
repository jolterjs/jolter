#!/bin/sh
# Jolter Installer for Linux and macOS
# https://jolter.dev

set -eu

# Color output helpers
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != "dumb" ]; then
  BOLD="$(tput bold 2>/dev/null || printf '')"
  GREEN="$(tput setaf 2 2>/dev/null || printf '')"
  CYAN="$(tput setaf 6 2>/dev/null || printf '')"
  YELLOW="$(tput setaf 3 2>/dev/null || printf '')"
  RED="$(tput setaf 1 2>/dev/null || printf '')"
  RESET="$(tput sgr0 2>/dev/null || printf '')"
else
  BOLD=""
  GREEN=""
  CYAN=""
  YELLOW=""
  RED=""
  RESET=""
fi

info() {
  printf "%s[info]%s %s\n" "${CYAN}" "${RESET}" "$1"
}

success() {
  printf "%s[success]%s %s\n" "${GREEN}" "${RESET}" "$1"
}

warn() {
  printf "%s[warning]%s %s\n" "${YELLOW}" "${RESET}" "$1"
}

error() {
  printf "%s[error]%s %s\n" "${RED}" "${RESET}" "$1" >&2
  exit 1
}

# Detect OS
OS="$(uname -s)"
case "${OS}" in
  Linux*)   PLATFORM="unknown-linux-gnu" ;;
  Darwin*)  PLATFORM="apple-darwin" ;;
  *)        error "Unsupported operating system: ${OS}. Jolter supports Linux and macOS." ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64)   ARCH_TARGET="x86_64" ;;
  aarch64|arm64)  ARCH_TARGET="aarch64" ;;
  *)              error "Unsupported architecture: ${ARCH}. Jolter supports x86_64 and aarch64 (ARM64)." ;;
esac

TARGET="${ARCH_TARGET}-${PLATFORM}"

# Resolve Jolter Home and Installation directories
JOLTER_HOME="${JOLTER_HOME:-$HOME/.jolter}"
JOLTER_BIN_DIR="${JOLTER_HOME}/bin"
JOLTER_SHIMS_DIR="${JOLTER_HOME}/shims"

# HTTP client helper
download_file() {
  url="$1"
  output="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$output" "$url"
  else
    error "Neither curl nor wget was found. Please install curl or wget to continue."
  fi
}

download_text() {
  url="$1"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- "$url"
  else
    error "Neither curl nor wget was found."
  fi
}

# Parse flags and environment variables
CHANNEL="${JOLTER_CHANNEL:-stable}"
if [ "${JOLTER_NIGHTLY:-0}" = "1" ] || [ "${JOLTER_NIGHTLY:-}" = "true" ]; then
  CHANNEL="nightly"
fi

for arg in "$@"; do
  case "$arg" in
    --nightly|-n)
      CHANNEL="nightly"
      ;;
    --channel=*)
      CHANNEL="${arg#*=}"
      ;;
  esac
done

# Version resolution
if [ -n "${JOLTER_VERSION:-}" ]; then
  VERSION="${JOLTER_VERSION}"
  case "${VERSION}" in
    v*) ;;
    *)  VERSION="v${VERSION}" ;;
  esac
  info "Installing specified Jolter version ${VERSION} for ${TARGET}..."
elif [ "${CHANNEL}" = "nightly" ]; then
  info "Fetching latest Jolter nightly release version..."
  RELEASES_JSON="$(download_text "https://api.github.com/repos/jolterjs/jolter/releases" 2>/dev/null || printf '')"
  VERSION="$(printf '%s' "${RELEASES_JSON}" | grep '"tag_name":' | grep 'nightly' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' | head -n 1)"
  if [ -z "${VERSION}" ]; then
    RELEASE_JSON="$(download_text "https://api.github.com/repos/jolterjs/jolter/releases/tags/nightly" 2>/dev/null || printf '')"
    VERSION="$(printf '%s' "${RELEASE_JSON}" | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' | head -n 1)"
  fi
  if [ -z "${VERSION}" ]; then
    VERSION="nightly"
  fi
  info "Installing Jolter nightly release ${VERSION} for ${TARGET}..."
else
  info "Fetching latest Jolter release version..."
  RELEASE_JSON="$(download_text "https://api.github.com/repos/jolterjs/jolter/releases/latest" 2>/dev/null || printf '')"
  VERSION="$(printf '%s' "${RELEASE_JSON}" | grep '"tag_name":' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/' | head -n 1)"
  if [ -z "${VERSION}" ]; then
    VERSION="v0.3.0"
    warn "Could not query latest release tag from GitHub API, falling back to ${VERSION}."
  fi
  info "Installing Jolter ${VERSION} for ${TARGET}..."
fi

# Prepare temporary working directory
TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'jolter-install')"
cleanup() {
  rm -rf "${TMP_DIR}"
}
trap cleanup EXIT INT TERM

ARCHIVE_NAME="jolter-${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/jolterjs/jolter/releases/download/${VERSION}/${ARCHIVE_NAME}"
CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

ARCHIVE_PATH="${TMP_DIR}/${ARCHIVE_NAME}"
CHECKSUM_PATH="${TMP_DIR}/${ARCHIVE_NAME}.sha256"

info "Downloading archive from ${DOWNLOAD_URL}..."
download_file "${DOWNLOAD_URL}" "${ARCHIVE_PATH}"
info "Downloading SHA-256 checksum file..."
download_file "${CHECKSUM_URL}" "${CHECKSUM_PATH}"

# SHA-256 Checksum verification
info "Verifying archive SHA-256 checksum..."
EXPECTED_SHA="$(cat "${CHECKSUM_PATH}" | awk '{print $1}')"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA="$(sha256sum "${ARCHIVE_PATH}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SHA="$(shasum -a 256 "${ARCHIVE_PATH}" | awk '{print $1}')"
elif command -v openssl >/dev/null 2>&1; then
  ACTUAL_SHA="$(openssl dgst -sha256 "${ARCHIVE_PATH}" | awk '{print $NF}')"
else
  warn "No SHA-256 tool found (sha256sum, shasum, openssl). Skipping checksum verification."
  ACTUAL_SHA="${EXPECTED_SHA}"
fi

if [ "${EXPECTED_SHA}" != "${ACTUAL_SHA}" ]; then
  error "Checksum verification failed!\nExpected: ${EXPECTED_SHA}\nActual:   ${ACTUAL_SHA}"
fi
success "SHA-256 checksum verified."

# Extract archive
info "Extracting ${ARCHIVE_NAME}..."
tar -xzf "${ARCHIVE_PATH}" -C "${TMP_DIR}"

# Locate extracted binary
EXTRACTED_DIR="${TMP_DIR}/jolter-${VERSION}-${TARGET}"
if [ ! -d "${EXTRACTED_DIR}" ]; then
  # Fallback search if directory name differs
  EXTRACTED_DIR="$(find "${TMP_DIR}" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
fi

if [ ! -f "${EXTRACTED_DIR}/jolter" ]; then
  error "Failed to locate jolter binary in extracted archive."
fi

# Ensure target directories exist
mkdir -p "${JOLTER_BIN_DIR}" "${JOLTER_SHIMS_DIR}"

# Copy binary to destination
cp "${EXTRACTED_DIR}/jolter" "${JOLTER_BIN_DIR}/jolter"
chmod +x "${JOLTER_BIN_DIR}/jolter"
if [ -f "${EXTRACTED_DIR}/jolter-shim" ]; then
  cp "${EXTRACTED_DIR}/jolter-shim" "${JOLTER_BIN_DIR}/jolter-shim"
  chmod +x "${JOLTER_BIN_DIR}/jolter-shim"
fi

success "Jolter binary installed to ${JOLTER_BIN_DIR}/jolter"

# Automatically prepend JOLTER_BIN_DIR and JOLTER_SHIMS_DIR at the top of the PATH in shell configuration files
PATH_LINE="export PATH=\"${JOLTER_BIN_DIR}:${JOLTER_SHIMS_DIR}:\$PATH\""
COMMENT_LINE="# Jolter environment setup"

add_to_profile() {
  profile_file="$1"
  if [ -f "${profile_file}" ]; then
    if ! grep -q "${JOLTER_BIN_DIR}" "${profile_file}" 2>/dev/null; then
      info "Adding Jolter bin and shims to top of PATH in ${profile_file}..."
      printf "\n%s\n%s\n" "${COMMENT_LINE}" "${PATH_LINE}" >> "${profile_file}"
    fi
  fi
}

PROFILE_UPDATED=0
for prof in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.profile"; do
  if [ -f "$prof" ]; then
    add_to_profile "$prof"
    PROFILE_UPDATED=1
  fi
done

# If no profile existed, create ~/.profile
if [ "$PROFILE_UPDATED" -eq 0 ]; then
  info "Creating $HOME/.profile and adding Jolter to PATH..."
  printf "%s\n%s\n" "${COMMENT_LINE}" "${PATH_LINE}" > "$HOME/.profile"
fi

# Fish shell support
FISH_CONFIG="$HOME/.config/fish/config.fish"
if [ -d "$HOME/.config/fish" ] || command -v fish >/dev/null 2>&1; then
  mkdir -p "$HOME/.config/fish"
  if [ -f "$FISH_CONFIG" ]; then
    if ! grep -q "${JOLTER_BIN_DIR}" "$FISH_CONFIG" 2>/dev/null; then
      info "Adding Jolter bin and shims to Fish PATH in ${FISH_CONFIG}..."
      printf "\n# Jolter environment setup\nfish_add_path --prepend \"%s\" \"%s\"\n" "${JOLTER_BIN_DIR}" "${JOLTER_SHIMS_DIR}" >> "$FISH_CONFIG"
    fi
  else
    printf "# Jolter environment setup\nfish_add_path --prepend \"%s\" \"%s\"\n" "${JOLTER_BIN_DIR}" "${JOLTER_SHIMS_DIR}" > "$FISH_CONFIG"
  fi
fi

# Run setup to initialize command shims
info "Running jolter setup..."
"${JOLTER_BIN_DIR}/jolter" setup --shell auto || warn "Initial setup completed with warnings."

# Final message to user
echo ""
printf "%s=======================================================%s\n" "${BOLD}${GREEN}" "${RESET}"
printf "%s   Jolter ${VERSION} was successfully installed!%s\n" "${BOLD}${GREEN}" "${RESET}"
printf "%s=======================================================%s\n" "${BOLD}${GREEN}" "${RESET}"
echo ""
printf "Binary location: %s/jolter\n" "${JOLTER_BIN_DIR}"
printf "Shims location:  %s\n" "${JOLTER_SHIMS_DIR}"
echo ""
printf "%sIMPORTANT: Please reload your shell to update your PATH:%s\n" "${BOLD}${YELLOW}" "${RESET}"
echo ""
if [ -n "${SHELL:-}" ]; then
  case "${SHELL}" in
    *zsh)  printf "  %s source ~/.zshrc %s (or: exec zsh)\n" "${BOLD}" "${RESET}" ;;
    *bash) printf "  %s source ~/.bashrc %s (or: exec bash)\n" "${BOLD}" "${RESET}" ;;
    *fish) printf "  %s source ~/.config/fish/config.fish %s\n" "${BOLD}" "${RESET}" ;;
    *)     printf "  %s source ~/.profile %s (or: exec \$SHELL)\n" "${BOLD}" "${RESET}" ;;
  esac
else
  printf "  %s source ~/.profile %s (or: exec \$SHELL)\n" "${BOLD}" "${RESET}"
fi
echo ""
printf "Then verify installation by running:\n"
printf "  %sjolter --version%s\n" "${BOLD}" "${RESET}"
printf "  %sjolter doctor%s\n" "${BOLD}" "${RESET}"
echo ""
