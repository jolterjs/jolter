#!/bin/sh
# Jolter Uninstaller for Linux and macOS
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

# Resolve Jolter Home and Installation directories
JOLTER_HOME="${JOLTER_HOME:-$HOME/.jolter}"
JOLTER_BIN_DIR="${JOLTER_HOME}/bin"
JOLTER_SHIMS_DIR="${JOLTER_HOME}/shims"

info "Uninstalling Jolter from ${JOLTER_HOME}..."

# Remove PATH additions from profile files
clean_profile() {
  profile_file="$1"
  if [ -f "${profile_file}" ]; then
    if grep -q "Jolter environment setup\|${JOLTER_HOME}" "${profile_file}" 2>/dev/null; then
      info "Removing Jolter PATH entries from ${profile_file}..."
      # Create backup
      cp "${profile_file}" "${profile_file}.jolter-bak"
      # Remove Jolter comment and export lines
      grep -v "# Jolter environment setup" "${profile_file}.jolter-bak" | \
      grep -v "${JOLTER_HOME}" | \
      grep -v "\.jolter/bin" | \
      grep -v "\.jolter/shims" > "${profile_file}" || true
      rm -f "${profile_file}.jolter-bak"
      success "Cleaned ${profile_file}"
    fi
  fi
}

for prof in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.profile"; do
  clean_profile "$prof"
done

# Clean Fish config if present
FISH_CONFIG="$HOME/.config/fish/config.fish"
if [ -f "${FISH_CONFIG}" ]; then
  if grep -q "Jolter environment setup\|${JOLTER_HOME}" "${FISH_CONFIG}" 2>/dev/null; then
    info "Removing Jolter PATH entries from ${FISH_CONFIG}..."
    cp "${FISH_CONFIG}" "${FISH_CONFIG}.jolter-bak"
    grep -v "# Jolter environment setup" "${FISH_CONFIG}.jolter-bak" | \
    grep -v "${JOLTER_HOME}" | \
    grep -v "\.jolter/bin" | \
    grep -v "\.jolter/shims" > "${FISH_CONFIG}" || true
    rm -f "${FISH_CONFIG}.jolter-bak"
    success "Cleaned ${FISH_CONFIG}"
  fi
fi

# Remove Jolter storage directory
if [ -d "${JOLTER_HOME}" ]; then
  info "Removing directory ${JOLTER_HOME} (includes runtimes, tools, shims, cache)..."
  rm -rf "${JOLTER_HOME}"
  success "Removed ${JOLTER_HOME}"
else
  warn "Jolter directory ${JOLTER_HOME} was not found."
fi

echo ""
printf "%s=======================================================%s\n" "${BOLD}${GREEN}" "${RESET}"
printf "%s   Jolter has been successfully uninstalled.%s\n" "${BOLD}${GREEN}" "${RESET}"
printf "%s=======================================================%s\n" "${BOLD}${GREEN}" "${RESET}"
echo ""
printf "Note: Project 'jolter.json' configuration files were preserved.\n"
echo ""
printf "%sPlease reload your shell to complete PATH updates:%s\n" "${BOLD}${YELLOW}" "${RESET}"
if [ -n "${SHELL:-}" ]; then
  case "${SHELL}" in
    *zsh)  printf "  %ssource ~/.zshrc%s (or: exec zsh)\n" "${BOLD}" "${RESET}" ;;
    *bash) printf "  %ssource ~/.bashrc%s (or: exec bash)\n" "${BOLD}" "${RESET}" ;;
    *fish) printf "  %ssource ~/.config/fish/config.fish%s\n" "${BOLD}" "${RESET}" ;;
    *)     printf "  %ssource ~/.profile%s (or: exec \$SHELL)\n" "${BOLD}" "${RESET}" ;;
  esac
else
  printf "  %ssource ~/.profile%s (or: exec \$SHELL)\n" "${BOLD}" "${RESET}"
fi
echo ""
