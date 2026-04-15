param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$SkipCargoBuild
)

$ErrorActionPreference = "Stop"

$VersionStateDir = Join-Path $PSScriptRoot ".build\versioning"
$LastSuccessfulVersionFile = Join-Path $VersionStateDir "last-successful-version"

function Get-BaseVersion {
    param([string]$RootDir)
    $cargoToml = Get-Content -Path (Join-Path $RootDir "Cargo.toml") -Raw
    $match = [regex]::Match($cargoToml, '(?ms)^\[package\].*?^version = "([^"]+)"')
    if (-not $match.Success) {
        throw "Could not resolve package version from Cargo.toml"
    }
    return $match.Groups[1].Value
}

function Test-CleanSemver {
    param([string]$Version)
    return $Version -match '^\d+\.\d+\.\d+$'
}

function Compare-Semver {
    param([string]$Left, [string]$Right)
    $l = $Left.Split('.') | ForEach-Object { [int]$_ }
    $r = $Right.Split('.') | ForEach-Object { [int]$_ }
    for ($i = 0; $i -lt 3; $i++) {
        if ($l[$i] -gt $r[$i]) { return 1 }
        if ($l[$i] -lt $r[$i]) { return -1 }
    }
    return 0
}

function Test-SameMajorMinor {
    param([string]$Left, [string]$Right)
    $l = $Left.Split('.')
    $r = $Right.Split('.')
    return ($l[0] -eq $r[0]) -and ($l[1] -eq $r[1])
}

function Step-PatchVersion {
    param([string]$Version)
    $parts = $Version.Split('.')
    $parts[2] = [string]([int]$parts[2] + 1)
    return $parts -join '.'
}

function Get-PackageVersion {
    param([string]$RootDir)

    if ($PackageVersion) {
        return $PackageVersion
    }

    $base = Get-BaseVersion -RootDir $RootDir
    if (-not (Test-CleanSemver $base)) {
        throw "Package version in Cargo.toml must be a clean semver like 0.2.0"
    }

    $last = $null
    if (Test-Path $LastSuccessfulVersionFile) {
        $last = (Get-Content -Path $LastSuccessfulVersionFile -Raw).Trim()
    }

    if ($last -and (Test-CleanSemver $last) -and (Test-SameMajorMinor $last $base) -and ((Compare-Semver $last $base) -ge 0)) {
        return Step-PatchVersion $last
    }

    return $base
}

function Save-SuccessfulBuildVersion {
    param([string]$Version)
    New-Item -ItemType Directory -Force -Path $VersionStateDir | Out-Null
    Set-Content -Path $LastSuccessfulVersionFile -Value $Version -NoNewline
}

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$TargetTriple = "x86_64-pc-windows-msvc"
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RootDir "target" }
$BinaryPath = Join-Path $TargetDir "$TargetTriple\release\terminaltiler.exe"
$StageRoot = Join-Path $RootDir "packaging\.build\windows-stage"
$PortableRoot = Join-Path $StageRoot "portable"
$DistDir = Join-Path $RootDir "dist"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$ZipLatestPath = Join-Path $DistDir "TerminalTiler-latest-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$InstallerLatestPath = Join-Path $DistDir "TerminalTiler-setup-latest-x86_64.exe"
$NsisScript = Join-Path $RootDir "packaging\windows\installer.nsi"

if (-not $SkipCargoBuild) {
    Write-Host "==> building Windows release binary"
    cargo build --release --target $TargetTriple --manifest-path (Join-Path $RootDir "Cargo.toml")
} else {
    Write-Host "==> using existing Windows release binary"
}

if (-not (Test-Path $BinaryPath)) {
    throw "Expected Windows binary was not found at $BinaryPath"
}

Write-Host "==> staging Windows release payload for version $ResolvedVersion"
Remove-Item -Recurse -Force $StageRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $PortableRoot | Out-Null
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Copy-Item -Path $BinaryPath -Destination (Join-Path $PortableRoot "TerminalTiler.exe")

$ReadmePath = Join-Path $PortableRoot "README-windows.txt"
@"
TerminalTiler for Windows
=========================

Runtime selection:
- WSL2 is preferred when a valid distro is available.
- TerminalTiler falls back to PowerShell when WSL2 is unavailable.

Browser tiles:
- Web tiles require Microsoft Edge WebView2 Runtime (Evergreen).
- Install it before opening any preset or restored session that includes browser tiles.
- Download: https://go.microsoft.com/fwlink/p/?LinkId=2124703

Launch:
- Run TerminalTiler.exe
- The native launcher and workspace host are both included in this build.

Support:
- Windows 11 is the supported Windows target.
"@ | Set-Content -Path $ReadmePath -Encoding ASCII

Write-Host "==> creating portable zip"
Remove-Item -Force $ZipPath, $ZipLatestPath -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $PortableRoot "*") -DestinationPath $ZipPath -Force
Copy-Item -Path $ZipPath -Destination $ZipLatestPath -Force

$Makensis = Get-Command makensis -ErrorAction SilentlyContinue
if ($Makensis) {
    Write-Host "==> building NSIS installer"
    Remove-Item -Force $InstallerPath, $InstallerLatestPath -ErrorAction SilentlyContinue
    & $Makensis.Source `
        "/DAPP_VERSION=$ResolvedVersion" `
        "/DSTAGE_DIR=$PortableRoot" `
        "/DOUT_FILE=$InstallerPath" `
        $NsisScript

    if (-not (Test-Path $InstallerPath)) {
        throw "Expected installer was not created at $InstallerPath"
    }

    Copy-Item -Path $InstallerPath -Destination $InstallerLatestPath -Force
} else {
    Write-Host "==> NSIS not found - skipping installer build"
    Write-Host "    To build the installer, install NSIS from https://nsis.sourceforge.io/"
}

Save-SuccessfulBuildVersion -Version $ResolvedVersion
Write-Host "Windows packaging complete"
Write-Host "  zip: $ZipPath"
if ($Makensis) {
    Write-Host "  installer: $InstallerPath"
}
