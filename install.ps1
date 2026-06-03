# install.ps1 — codesynapse installer for Windows (x86_64)
# Usage: irm https://raw.githubusercontent.com/sohilladhani/codesynapse/master/install.ps1 | iex
param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Repo = "sohilladhani/codesynapse"
$BinName = "codesynapse.exe"
$Asset = "codesynapse-windows-x86_64.exe"

# Resolve version
if (-not $Version) {
    $release = Invoke-RestMethod "https://api.github.com/repos/$Repo/releases/latest"
    $Version = $release.tag_name
}

if (-not $Version) {
    Write-Error "Failed to resolve latest release version."
    exit 1
}

$Url = "https://github.com/$Repo/releases/download/$Version/$Asset"

Write-Host "Installing codesynapse $Version..."

# Download
$TmpFile = Join-Path $env:TEMP $BinName
Invoke-WebRequest -Uri $Url -OutFile $TmpFile -UseBasicParsing

# Install — prefer a user-writable location on PATH
$InstallDir = "$env:LOCALAPPDATA\Programs\codesynapse"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item $TmpFile "$InstallDir\$BinName" -Force
Remove-Item $TmpFile -Force

# Add to user PATH if not already present
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$UserPath;$InstallDir", "User")
    Write-Host ""
    Write-Host "Added $InstallDir to your PATH."
    Write-Host "Restart your terminal for PATH changes to take effect."
}

Write-Host ""
Write-Host "Installed: $InstallDir\$BinName"
Write-Host ""
Write-Host "Next steps (in a new terminal):"
Write-Host "  codesynapse setup                        # download embedding model (~62MB)"
Write-Host "  codesynapse module add myrepo C:\path\to\repo"
Write-Host "  codesynapse setup --client claude        # wire up Claude Code MCP"
Write-Host "  codesynapse setup --client cursor        # wire up Cursor MCP"
Write-Host ""
Write-Host "Docs: https://github.com/$Repo"
