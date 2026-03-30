param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$SkipCargoBuild
)

$ErrorActionPreference = "Stop"

function Get-PackageVersion {
    param([string]$RootDir)

    if ($PackageVersion) {
        return $PackageVersion
    }

    $cargoToml = Get-Content -Path (Join-Path $RootDir "Cargo.toml") -Raw
    $match = [regex]::Match($cargoToml, '(?ms)^\[package\].*?^version = "([^"]+)"')
    if (-not $match.Success) {
        throw "Could not resolve package version from Cargo.toml"
    }

    return $match.Groups[1].Value
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

$Makensis = Get-Command makensis -ErrorAction Stop
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

Write-Host "Windows packaging complete"
Write-Host "  zip: $ZipPath"
Write-Host "  installer: $InstallerPath"
