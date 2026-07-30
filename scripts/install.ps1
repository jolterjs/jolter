# Jolter Installer for Windows
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

function Write-Err($Message) {
    Write-Host "[error] " -NoNewline -ForegroundColor Red
    Write-Host $Message
    exit 1
}

# OS Check
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Write-Err "install.ps1 is intended for Windows systems."
}

# Architecture Detection
$rawArch = $env:PROCESSOR_ARCHITECTURE
if ($env:PROCESSOR_ARCHITEW6432) {
    $rawArch = $env:PROCESSOR_ARCHITEW6432
}

switch -regex ($rawArch) {
    'AMD64|x86_64' { $target = 'x86_64-pc-windows-msvc' }
    'ARM64|aarch64' { $target = 'aarch64-pc-windows-msvc' }
    default { Write-Err "Unsupported Windows architecture: $rawArch. Jolter supports x64 and ARM64." }
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

# Enable TLS 1.2 / 1.3 for web requests
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 -bor [Net.SecurityProtocolType]::Tls13

# Version Resolution
if ($env:JOLTER_VERSION) {
    $version = $env:JOLTER_VERSION
    if (-not $version.StartsWith("v")) {
        $version = "v$version"
    }
    Write-Info "Installing specified Jolter version $version for $target..."
} else {
    Write-Info "Fetching latest Jolter release version..."
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/jolterjs/jolter/releases/latest" -Headers @{ "User-Agent" = "jolter-installer" }
        $version = $release.tag_name
    } catch {
        $version = "v0.3.0"
        Write-Warn "Could not query latest release tag from GitHub API, falling back to $version."
    }
    Write-Info "Installing Jolter $version for $target..."
}

# Create temporary directory
$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("jolter-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

try {
    $archiveName = "jolter-$version-$target.zip"
    $downloadUrl = "https://github.com/jolterjs/jolter/releases/download/$version/$archiveName"
    $checksumUrl = "$downloadUrl.sha256"

    $archivePath = Join-Path $tempDir $archiveName
    $checksumPath = Join-Path $tempDir "$archiveName.sha256"

    Write-Info "Downloading release archive from $downloadUrl..."
    Invoke-WebRequest -Uri $downloadUrl -OutFile $archivePath -UseBasicParsing

    Write-Info "Downloading SHA-256 checksum..."
    Invoke-WebRequest -Uri $checksumUrl -OutFile $checksumPath -UseBasicParsing

    # Verify SHA-256 Checksum
    Write-Info "Verifying SHA-256 checksum..."
    $checksumContent = Get-Content -Path $checksumPath -Raw
    $expectedHash = ($checksumContent.Trim().Split(" `t")[0]).ToLowerInvariant()
    $actualHash = (Get-FileHash -Path $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()

    if ($expectedHash -ne $actualHash) {
        Write-Err "Checksum verification failed!`nExpected: $expectedHash`nActual:   $actualHash"
    }
    Write-Success "SHA-256 checksum verified."

    # Extract Archive
    Write-Info "Extracting $archiveName..."
    $extractDir = Join-Path $tempDir "extracted"
    Expand-Archive -Path $archivePath -DestinationPath $extractDir -Force

    $binarySource = Get-ChildItem -Path $extractDir -Recurse -Filter "jolter.exe" | Select-Object -First 1
    if (-not $binarySource) {
        Write-Err "Failed to locate jolter.exe in extracted archive."
    }

    # Ensure Target Directories Exist
    if (-not (Test-Path $jolterBinDir)) {
        New-Item -ItemType Directory -Path $jolterBinDir -Force | Out-Null
    }
    if (-not (Test-Path $jolterShimsDir)) {
        New-Item -ItemType Directory -Path $jolterShimsDir -Force | Out-Null
    }

    # Copy Binaries
    Copy-Item -Path $binarySource.FullName -Destination (Join-Path $jolterBinDir "jolter.exe") -Force

    $shimSource = Get-ChildItem -Path $extractDir -Recurse -Filter "jolter-shim.exe" | Select-Object -First 1
    if ($shimSource) {
        Copy-Item -Path $shimSource.FullName -Destination (Join-Path $jolterBinDir "jolter-shim.exe") -Force
    }

    Write-Success "Jolter binary installed to $jolterBinDir\jolter.exe"

    # Automatically prepend jolterBinDir and jolterShimsDir at the TOP of User PATH
    Write-Info "Configuring Windows User PATH environment variable..."
    $currentUserPath = [Environment]::GetEnvironmentVariable("Path", [EnvironmentVariableTarget]::User)
    
    $pathParts = if ($currentUserPath) { $currentUserPath.Split(';') } else { @() }
    
    # Remove any existing entries matching jolterBinDir or jolterShimsDir
    $filteredParts = $pathParts | Where-Object { 
        $_ -and $_.Trim() -and 
        $_.TrimEnd('\/') -ne $jolterBinDir.TrimEnd('\/') -and 
        $_.TrimEnd('\/') -ne $jolterShimsDir.TrimEnd('\/') 
    }

    # Prepend jolterBinDir and jolterShimsDir at the TOP
    $newParts = @($jolterBinDir, $jolterShimsDir) + $filteredParts
    $newUserPath = $newParts -join ';'

    [Environment]::SetEnvironmentVariable("Path", $newUserPath, [EnvironmentVariableTarget]::User)
    Write-Success "Added Jolter bin and shims to the top of User PATH environment variable."

    # Update process PATH for current PowerShell session
    $env:PATH = "$jolterBinDir;$jolterShimsDir;$env:PATH"

    # Execute jolter setup to initialize command shims
    Write-Info "Running jolter setup..."
    $installedExecutable = Join-Path $jolterBinDir "jolter.exe"
    & $installedExecutable setup --shell powershell

    # Final Output Message
    Write-Host ""
    Write-Host "=======================================================" -ForegroundColor Green
    Write-Host "   Jolter $version was successfully installed!" -ForegroundColor Green
    Write-Host "=======================================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Binary location: $jolterBinDir\jolter.exe"
    Write-Host "Shims location:  $jolterShimsDir"
    Write-Host ""
    Write-Host "IMPORTANT: Please reload your shell or open a new terminal window" -ForegroundColor Yellow
    Write-Host "to ensure your PATH changes take effect across all applications." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "Verify installation by running:"
    Write-Host "  jolter --version" -ForegroundColor Cyan
    Write-Host "  jolter doctor" -ForegroundColor Cyan
    Write-Host ""

} finally {
    if (Test-Path $tempDir) {
        Remove-Item -Path $tempDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}
