#!/bin/sh
# ved — installer shim
#
# Delegates to the cargo-dist-generated installer for the latest release.
# This exists so the install and uninstall one-liners share a URL shape:
#
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/ved/main/install.sh | sh
#     curl --proto '=https' --tlsv1.2 -LsSf https://raw.githubusercontent.com/excelano/ved/main/uninstall.sh | sh

set -eu

curl --proto '=https' --tlsv1.2 -LsSf \
    https://github.com/excelano/ved/releases/latest/download/ved-installer.sh | sh
