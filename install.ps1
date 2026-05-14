# ved — installer shim
#
# Delegates to the cargo-dist-generated installer for the latest release.
# This exists so the install and uninstall one-liners share a URL shape:
#
#     irm https://raw.githubusercontent.com/excelano/ved/main/install.ps1 | iex
#     irm https://raw.githubusercontent.com/excelano/ved/main/uninstall.ps1 | iex

$ErrorActionPreference = 'Stop'
irm https://github.com/excelano/ved/releases/latest/download/ved-installer.ps1 | iex
