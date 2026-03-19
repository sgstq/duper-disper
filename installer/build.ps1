# Build script for Windows (run on Windows or in CI)
# Usage: .\build.ps1 [-Release]

param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"

$version = (cargo metadata --no-deps --format-version 1 | ConvertFrom-Json).packages[0].version
Write-Host "Building Duper Disper v$version" -ForegroundColor Cyan

# Build
$profile = if ($Release) { "--release" } else { "" }
$targetDir = if ($Release) { "target\release" } else { "target\debug" }

Write-Host "Compiling ($( if ($Release) { 'release' } else { 'debug' } ))..."
cargo build $profile
if ($LASTEXITCODE -ne 0) { exit 1 }

# Create output directory
$outDir = "installer\output"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# Copy binary
Copy-Item "$targetDir\duper-disper.exe" "installer\duper-disper.exe" -Force

# Build NSIS installer
Write-Host "Building installer..."
$nsisArgs = "/DVERSION=$version", "installer\duper-disper.nsi"

if (Get-Command makensis -ErrorAction SilentlyContinue) {
    makensis @nsisArgs
} elseif (Test-Path "C:\Program Files (x86)\NSIS\makensis.exe") {
    & "C:\Program Files (x86)\NSIS\makensis.exe" @nsisArgs
} else {
    Write-Host "NSIS not found. Install from https://nsis.sourceforge.io/" -ForegroundColor Red
    Write-Host "Standalone binary available at: $targetDir\duper-disper.exe" -ForegroundColor Yellow
    exit 1
}

Move-Item "installer\duper-disper-setup.exe" "$outDir\duper-disper-$version-setup.exe" -Force

# Also create a portable ZIP
Write-Host "Creating portable ZIP..."
Compress-Archive -Path "$targetDir\duper-disper.exe" -DestinationPath "$outDir\duper-disper-$version-portable.zip" -Force

Write-Host ""
Write-Host "Build complete!" -ForegroundColor Green
Write-Host "  Installer: $outDir\duper-disper-$version-setup.exe"
Write-Host "  Portable:  $outDir\duper-disper-$version-portable.zip"
