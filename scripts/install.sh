#!/usr/bin/env bash
set -euo pipefail

echo "⚡ Installing WPG (Web Progressive Graphic) CLI..."

INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "${INSTALL_DIR}"

OS="$(uname -s)"
ARCH="$(uname -m)"

if [ -f "./target/release/wpg" ]; then
    cp "./target/release/wpg" "${INSTALL_DIR}/wpg"
    chmod +x "${INSTALL_DIR}/wpg"
    echo "✓ Installed local release binary to: ${INSTALL_DIR}/wpg"
else
    echo "Downloading latest release binary for ${OS}-${ARCH}..."
    DOWNLOAD_URL="https://github.com/wpg-format/wpg/releases/latest/download/wpg-${OS}-${ARCH}"
    curl -fsSL "${DOWNLOAD_URL}" -o "${INSTALL_DIR}/wpg"
    chmod +x "${INSTALL_DIR}/wpg"
fi

if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
    echo "Notice: Add ${INSTALL_DIR} to your PATH by adding this line to ~/.bashrc or ~/.zshrc:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo ""
echo "🎉 WPG is ready! Try running:"
echo "  wpg --help"
echo "  wpg serve --port 3000"
echo ""
