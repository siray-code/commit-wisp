param(
    [string]$Version = $(if ($env:COMMIT_WISP_VERSION) { $env:COMMIT_WISP_VERSION } else { "latest" }),
    [string]$InstallDir = $(if ($env:COMMIT_WISP_INSTALL_DIR) { $env:COMMIT_WISP_INSTALL_DIR } else { (Get-Location).Path }),
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

$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$target = switch ($architecture) {
    "X64" { "x86_64-pc-windows-msvc" }
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
    Write-Host "PATH was not changed. Run .\commit-wisp.exe from that directory."
} finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tempDir
}
