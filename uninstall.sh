#!/bin/sh
# ved — uninstaller
#
# Removes the ved binary installed by install.sh. ved stores nothing else
# on disk (no config, no history), so this is the entire cleanup.
#
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/ved/main/uninstall.sh | sh

set -eu

if [ -n "${CARGO_HOME:-}" ]; then
    install_dir="$CARGO_HOME/bin"
else
    install_dir="$HOME/.cargo/bin"
fi

target="$install_dir/ved"

if [ -e "$target" ]; then
    rm -f "$target"
    echo "Removed $target"
elif command -v ved >/dev/null 2>&1; then
    found="$(command -v ved)"
    echo "ved is installed at $found, not the expected location ($target)."
    echo "Remove it manually if you want it gone."
    exit 1
else
    echo "ved is not installed."
fi
