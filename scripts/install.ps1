param(
    [string]$Version = $(if ($env:COMMIT_WISP_VERSION) { $env:COMMIT_WISP_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:COMMIT_WISP_INSTALL_DIR) { $env:COMMIT_WISP_INSTALL_DIR } else { Join-Path ([Environment]::GetFolderPath("LocalApplicationData")) "Programs\commit-wisp\bin" }),
    [string]$Repository = $(if ($env:COMMIT_WISP_REPOSITORY) { $env:COMMIT_WISP_REPOSITORY } else { "siray-code/commit-wisp" })
)

$ErrorActionPreference = "Stop"
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

if ($Repository -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
    throw "Invalid repository name."
}

if ($Version -ne "latest") {
    if ($Version -notmatch '^[A-Za-z0-9._-]+$') {
        throw "Invalid version."
    }
    if (-not $Version.StartsWith("v")) {
        $Version = "v$Version"
    }
}

$architecture = $null
try {
    $runtimeArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($null -ne $runtimeArchitecture) {
        $architecture = $runtimeArchitecture.ToString()
    }
} catch {
    # RuntimeInformation.OSArchitecture is unavailable in some Windows PowerShell 5.1 environments.
}

if ([string]::IsNullOrWhiteSpace($architecture)) {
    $architecture = if ($env:PROCESSOR_ARCHITEW6432) {
        $env:PROCESSOR_ARCHITEW6432
    } else {
        $env:PROCESSOR_ARCHITECTURE
    }
}

$target = switch ($architecture) {
    "X64" { "x86_64-pc-windows-msvc" }
    "AMD64" { "x86_64-pc-windows-msvc" }
    "Arm64" { "aarch64-pc-windows-msvc" }
    default { throw "Unsupported Windows architecture: $architecture (supported: x64, arm64)" }
}

$asset = "commit-wisp-$target.zip"
$baseUrl = if ($Version -eq "latest") {
    "https://github.com/$Repository/releases/latest/download"
} else {
    "https://github.com/$Repository/releases/download/$Version"
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("commit-wisp-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tempDir | Out-Null

try {
    $archivePath = Join-Path $tempDir $asset
    $checksumsPath = Join-Path $tempDir "SHA256SUMS"

    Write-Host "Downloading $asset..."
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/$asset" -OutFile $archivePath
    Invoke-WebRequest -UseBasicParsing -Uri "$baseUrl/SHA256SUMS" -OutFile $checksumsPath

    $assetPattern = [regex]::Escape($asset)
    $checksumLine = Get-Content $checksumsPath |
        Where-Object { $_ -match ("^[0-9a-fA-F]{64}\s+\*?" + $assetPattern + "$") } |
        Select-Object -First 1
    if (-not $checksumLine) {
        throw "$asset is missing from SHA256SUMS."
    }

    $expected = ($checksumLine -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $archivePath).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Checksum verification failed for $asset."
    }

    Expand-Archive -LiteralPath $archivePath -DestinationPath $tempDir -Force
    $binaryPath = Join-Path $tempDir "commit-wisp-$target\commit-wisp.exe"
    if (-not (Test-Path -LiteralPath $binaryPath -PathType Leaf)) {
        throw "Archive does not contain commit-wisp.exe."
    }

    $destination = [System.IO.Path]::GetFullPath($InstallDir)
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    Copy-Item -LiteralPath $binaryPath -Destination (Join-Path $destination "commit-wisp.exe") -Force

    Write-Host "Installed commit-wisp to $destination\commit-wisp.exe"

    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $pathEntries = @($userPath -split ';' | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    $destinationInPath = $pathEntries | Where-Object {
        [string]::Equals($_.Trim().TrimEnd('\'), $destination.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    }
    if (-not $destinationInPath) {
        $newUserPath = (@($pathEntries) + $destination) -join ';'
        [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        Write-Host "Added $destination to the user PATH."
    } else {
        Write-Host "$destination is already in the user PATH."
    }

    $processPathEntries = @($env:Path -split ';')
    $destinationInProcessPath = $processPathEntries | Where-Object {
        [string]::Equals($_.Trim().TrimEnd('\'), $destination.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    }
    if (-not $destinationInProcessPath) {
        $env:Path = "$destination;$env:Path"
    }
    Write-Host "Run commit-wisp setup to get started."
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tempDir
}
