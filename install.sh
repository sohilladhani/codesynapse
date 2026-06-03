#!/usr/bin/env sh
set -e

REPO="sohilladhani/codesynapse"
BIN="codesynapse"

# Detect OS and arch
OS="$(uname -s)"
ARCH="$(uname -m)"

case "${OS}" in
  Linux)
    case "${ARCH}" in
      x86_64) ASSET="codesynapse-linux-x86_64" ;;
      *)
        echo "Unsupported Linux architecture: ${ARCH}"
        echo "Build from source: https://github.com/${REPO}#building-from-source"
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "${ARCH}" in
      arm64)  ASSET="codesynapse-macos-aarch64" ;;
      x86_64) ASSET="codesynapse-macos-aarch64" ;; # Rosetta 2 fallback
      *)
        echo "Unsupported macOS architecture: ${ARCH}"
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Unsupported OS: ${OS}"
    echo "Build from source: https://github.com/${REPO}#building-from-source"
    exit 1
    ;;
esac

# Resolve version
if [ -z "${VERSION}" ]; then
  VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
fi

if [ -z "${VERSION}" ]; then
  echo "Failed to resolve latest release version."
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

echo "Installing codesynapse ${VERSION} (${ASSET})..."

# Download
TMP="$(mktemp)"
curl -fsSL --progress-bar "${URL}" -o "${TMP}"
chmod +x "${TMP}"

# Install
if [ -w /usr/local/bin ]; then
  mv "${TMP}" "/usr/local/bin/${BIN}"
  INSTALL_DIR="/usr/local/bin"
elif command -v sudo >/dev/null 2>&1 && sudo -n mv "${TMP}" "/usr/local/bin/${BIN}" 2>/dev/null; then
  INSTALL_DIR="/usr/local/bin"
else
  INSTALL_DIR="${HOME}/.local/bin"
  mkdir -p "${INSTALL_DIR}"
  mv "${TMP}" "${INSTALL_DIR}/${BIN}"
  # Remind user to add to PATH if not already there
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      echo ""
      echo "Add to PATH (then restart your shell):"
      echo '  echo '"'"'export PATH="$HOME/.local/bin:$PATH"'"'"' >> ~/.bashrc   # bash'
      echo '  echo '"'"'export PATH="$HOME/.local/bin:$PATH"'"'"' >> ~/.zshrc    # zsh'
      ;;
  esac
fi

echo ""
echo "Installed: ${INSTALL_DIR}/${BIN}"
echo ""
echo "Next steps:"
echo "  codesynapse setup                        # download embedding model (~62MB)"
echo "  codesynapse module add myrepo /path/to/repo"
echo "  codesynapse setup --client claude        # wire up Claude Code MCP"
echo "  codesynapse setup --client cursor        # wire up Cursor MCP"
echo ""
echo "Docs: https://github.com/${REPO}"
