# Chameleon one-command installer for Windows
# Usage: irm https://raw.githubusercontent.com/Haroon966/Chameleon/main/install.ps1 | iex
# Or:    Invoke-RestMethod ... | Invoke-Expression
# Set $env:CHAMELEON_GITHUB_REPO = "Haroon966/Chameleon" if different.

$ErrorActionPreference = "Stop"
$repo = if ($env:CHAMELEON_GITHUB_REPO) { $env:CHAMELEON_GITHUB_REPO } else { "Haroon966/Chameleon" }
$assetPattern = "x86_64-pc-windows-msvc.zip"

Write-Host "Fetching latest release from GitHub ($repo)..."

$releaseUrl = "https://api.github.com/repos/$repo/releases/latest"
try {
    $release = Invoke-RestMethod -Uri $releaseUrl -Headers @{ "Accept" = "application/vnd.github.v3+json" }
} catch {
    Write-Error "Could not fetch release. Check network and that a release exists for $repo"
    exit 1
}

$asset = $release.assets | Where-Object { $_.name -like "*$assetPattern*" } | Select-Object -First 1
if (-not $asset) {
    Write-Error "No Windows asset found for this release (looking for *$assetPattern*)."
    exit 1
}

$version = $release.tag_name
Write-Host "Installing Chameleon $version..."

$tempDir = Join-Path $env:TEMP "chameleon-install"
if (Test-Path $tempDir) { Remove-Item -Recurse -Force $tempDir }
New-Item -ItemType Directory -Path $tempDir | Out-Null

$zipPath = Join-Path $tempDir "chameleon.zip"
Write-Host "Downloading..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing

Expand-Archive -Path $zipPath -DestinationPath $tempDir -Force
$extractedDir = Get-ChildItem -Path $tempDir -Directory | Select-Object -First 1
$exePath = Join-Path $extractedDir.FullName "chameleon.exe"
if (-not (Test-Path $exePath)) {
    Write-Error "chameleon.exe not found in archive."
    exit 1
}

# Install location: LOCALAPPDATA\Programs\Chameleon (user install, no admin)
$installDir = Join-Path $env:LOCALAPPDATA "Programs\Chameleon"
if (-not (Test-Path $installDir)) {
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
}

$targetExe = Join-Path $installDir "chameleon.exe"
Copy-Item -Path $exePath -Destination $targetExe -Force
Write-Host "Installed: $targetExe"

# Suggest adding to PATH if not already
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$dirToAdd = $installDir
if ($userPath -notlike "*$dirToAdd*") {
    Write-Host ""
    Write-Host "Add Chameleon to PATH? (recommended) [Y/n]: " -NoNewline
    $reply = Read-Host
    if ($reply -eq "" -or $reply -match "^[yY]") {
        [Environment]::SetEnvironmentVariable("Path", "$userPath;$dirToAdd", "User")
        $env:Path = "$env:Path;$dirToAdd"
        Write-Host "Added to PATH. You may need to restart the terminal."
    }
}

Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
Write-Host ""
Write-Host "Done. Run 'chameleon' from a new terminal (or use the full path: $targetExe)"
