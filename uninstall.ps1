# ved — uninstaller
#
# Removes the ved binary installed by install.ps1. ved stores nothing else
# on disk (no config, no history), so this is the entire cleanup.
#
#     irm https://raw.githubusercontent.com/excelano/ved/main/uninstall.ps1 | iex

$ErrorActionPreference = 'Stop'

$installDir = if ($env:CARGO_HOME) {
    Join-Path $env:CARGO_HOME 'bin'
} else {
    Join-Path $env:USERPROFILE '.cargo\bin'
}
$target = Join-Path $installDir 'ved.exe'

if (Test-Path $target) {
    Remove-Item $target
    Write-Host "Removed $target"
} else {
    $existing = Get-Command ved -ErrorAction SilentlyContinue
    if ($existing) {
        Write-Host "ved is installed at $($existing.Source), not the expected location ($target)."
        Write-Host "Remove it manually if you want it gone."
        exit 1
    } else {
        Write-Host "ved is not installed."
    }
}
