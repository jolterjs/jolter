# Jolter Uninstaller for Windows
# https://jolter.dev

$ErrorActionPreference = 'Stop'

function Write-Info($Message) {
    Write-Host "[info] " -NoNewline -ForegroundColor Cyan
    Write-Host $Message
}

function Write-Success($Message) {
    Write-Host "[success] " -NoNewline -ForegroundColor Green
    Write-Host $Message
}

function Write-Warn($Message) {
    Write-Host "[warning] " -NoNewline -ForegroundColor Yellow
    Write-Host $Message
}

# Resolve Jolter Home and Installation directories
if ($env:JOLTER_HOME) {
    $jolterHome = $env:JOLTER_HOME
} elseif ($env:USERPROFILE) {
    $jolterHome = Join-Path $env:USERPROFILE ".jolter"
} else {
    $jolterHome = Join-Path $HOME ".jolter"
}

$jolterBinDir = Join-Path $jolterHome "bin"
$jolterShimsDir = Join-Path $jolterHome "shims"

Write-Info "Uninstalling Jolter from $jolterHome..."

# Remove Jolter paths from Windows User PATH environment variable
Write-Info "Cleaning Windows User PATH environment variable..."
$currentUserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)

if ($currentUserPath) {
    $pathParts = $currentUserPath.Split(';')
    $cleanedParts = $pathParts | Where-Object {
        if (-not $_ -or -not $_.Trim()) { return $false }
        $trimmed = $_.Trim().TrimEnd('\/')
        return ($trimmed -ne $jolterHome.TrimEnd('\/')) -and 
               ($trimmed -ne $jolterBinDir.TrimEnd('\/')) -and 
               ($trimmed -ne $jolterShimsDir.TrimEnd('\/')) -and 
               (-not ($trimmed.EndsWith("\.jolter\bin") -or $trimmed.EndsWith("\.jolter\shims")))
    }
    
    $cleanedPath = $cleanedParts -join ';'
    [Environment]::SetEnvironmentVariable("Path", $cleanedPath, [EnvironmentVariableTarget]::User)
    Write-Success "Removed Jolter bin and shims from User PATH."
}

# Remove Jolter storage directory
if (Test-Path $jolterHome) {
    Write-Info "Removing directory $jolterHome (includes runtimes, tools, shims, cache)..."
    try {
        Remove-Item -Path $jolterHome -Recurse -Force
        Write-Success "Removed $jolterHome"
    } catch {
        Write-Warn "Could not completely remove $jolterHome: $_"
    }
} else {
    Write-Warn "Jolter directory $jolterHome was not found."
}

Write-Host ""
Write-Host "=======================================================" -ForegroundColor Green
Write-Host "   Jolter has been successfully uninstalled." -ForegroundColor Green
Write-Host "=======================================================" -ForegroundColor Green
Write-Host ""
Write-Host "Note: Project 'jolter.json' configuration files were preserved."
Write-Host ""
Write-Host "Please restart your terminal windows to finalize PATH updates." -ForegroundColor Yellow
Write-Host ""
