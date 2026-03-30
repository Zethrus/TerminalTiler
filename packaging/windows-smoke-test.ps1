param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$SkipBuild
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

function Assert-Path {
    param([string]$Path, [string]$Description)

    if (-not (Test-Path $Path)) {
        throw "$Description was not found at $Path"
    }
}

function Invoke-LaunchSmoke {
    param([string]$ExePath)

    $process = Start-Process -FilePath $ExePath -PassThru
    Start-Sleep -Seconds 4
    if ($process.HasExited -and $process.ExitCode -ne 0) {
        throw "Process $ExePath exited with code $($process.ExitCode)"
    }
    if (-not $process.HasExited) {
        Stop-Process -Id $process.Id -Force
    }
}

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$DistDir = Join-Path $RootDir "dist"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$SmokeRoot = Join-Path $RootDir "packaging\.build\windows-smoke"
$PortableExtractRoot = Join-Path $SmokeRoot "portable"
$InstallRoot = Join-Path $SmokeRoot "install"

if (-not $SkipBuild) {
    & (Join-Path $RootDir "packaging\build-windows.ps1") -PackageVersion $ResolvedVersion
}

Write-Host "==> checking Windows release artifacts"
Assert-Path -Path $ZipPath -Description "Portable zip"
Assert-Path -Path $InstallerPath -Description "Installer"

Write-Host "==> extracting portable zip"
Remove-Item -Recurse -Force $SmokeRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $PortableExtractRoot | Out-Null
Expand-Archive -Path $ZipPath -DestinationPath $PortableExtractRoot -Force

$PortableExe = Join-Path $PortableExtractRoot "TerminalTiler.exe"
$PortableReadme = Join-Path $PortableExtractRoot "README-windows.txt"
Assert-Path -Path $PortableExe -Description "Portable executable"
Assert-Path -Path $PortableReadme -Description "Portable README"

Write-Host "==> smoke-launching portable executable"
Invoke-LaunchSmoke -ExePath $PortableExe

Write-Host "==> smoke-installing NSIS package"
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
$InstallerArgs = @("/S", "/D=$InstallRoot")
$InstallerProcess = Start-Process -FilePath $InstallerPath -ArgumentList $InstallerArgs -PassThru -Wait
if ($InstallerProcess.ExitCode -ne 0) {
    throw "Installer exited with code $($InstallerProcess.ExitCode)"
}

$InstalledExe = Join-Path $InstallRoot "TerminalTiler.exe"
$InstalledUninstaller = Join-Path $InstallRoot "Uninstall.exe"
Assert-Path -Path $InstalledExe -Description "Installed executable"
Assert-Path -Path $InstalledUninstaller -Description "Installed uninstaller"

Write-Host "==> smoke-launching installed executable"
Invoke-LaunchSmoke -ExePath $InstalledExe

Write-Host "Windows smoke test passed"
