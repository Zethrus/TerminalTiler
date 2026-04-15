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

function Convert-ToTomlPath {
    param([string]$Path)

    return ($Path -replace '\\', '\\\\')
}

function Initialize-SmokeProfile {
    param([string]$SandboxRoot)

    $workspaceRoot = Join-Path $SandboxRoot "workspace"
    $roamingRoot = Join-Path $SandboxRoot "AppData\Roaming"
    $localRoot = Join-Path $SandboxRoot "AppData\Local"
    $configTargets = @(
        Join-Path $roamingRoot "dev\Zethrus\TerminalTiler\config",
        Join-Path $roamingRoot "dev\Zethrus\TerminalTiler"
    )
    $dataTargets = @(
        Join-Path $roamingRoot "dev\Zethrus\TerminalTiler\data",
        Join-Path $roamingRoot "dev\Zethrus\TerminalTiler"
    )

    New-Item -ItemType Directory -Force -Path (Join-Path $workspaceRoot "src") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localRoot "dev\Zethrus\TerminalTiler\state\logs") | Out-Null

    foreach ($dir in $configTargets + $dataTargets) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }

    $preferences = @"
version = 1
default_restore_mode = "shell-only"
"@
    foreach ($configDir in $configTargets) {
        Set-Content -Path (Join-Path $configDir "preferences.toml") -Value $preferences -Encoding ASCII
    }

    $workspacePath = Convert-ToTomlPath -Path $workspaceRoot
    $session = @"
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspacePath"
custom_title = "Smoke Restore"
terminal_zoom_steps = 0

[tabs.preset]
id = "smoke-restore"
name = "Smoke Restore"
description = "Packaged restore smoke test"
tags = ["smoke", "restore"]
root_label = "Workspace root"
theme = "system"
density = "compact"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.5

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-smoke"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "tile"
id = "web-smoke"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-amber"
tile_kind = "web-view"
url = "https://example.com"

[tabs.preset.layout.second.working_directory]
type = "workspace-root"
"@
    foreach ($dataDir in $dataTargets) {
        Set-Content -Path (Join-Path $dataDir "session.toml") -Value $session -Encoding ASCII
    }

    return @{
        AppData = $roamingRoot
        LocalAppData = $localRoot
        UserProfile = Join-Path $SandboxRoot "User"
    }
}

function Find-SessionLog {
    param([string]$SandboxRoot)

    return Get-ChildItem -Path $SandboxRoot -Filter "terminaltiler-session.log" -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}

function Invoke-LaunchSmoke {
    param([string]$ExePath, [string]$SandboxRoot, [string]$Label)

    $profile = Initialize-SmokeProfile -SandboxRoot $SandboxRoot
    $previousEnvironment = @{
        APPDATA = $env:APPDATA
        LOCALAPPDATA = $env:LOCALAPPDATA
        USERPROFILE = $env:USERPROFILE
        HOME = $env:HOME
    }

    New-Item -ItemType Directory -Force -Path $profile.UserProfile | Out-Null
    $env:APPDATA = $profile.AppData
    $env:LOCALAPPDATA = $profile.LocalAppData
    $env:USERPROFILE = $profile.UserProfile
    $env:HOME = $profile.UserProfile

    try {
        $process = Start-Process -FilePath $ExePath -PassThru
        Start-Sleep -Seconds 6
        if ($process.HasExited -and $process.ExitCode -ne 0) {
            throw "Process $ExePath exited with code $($process.ExitCode)"
        }
        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
        }

        $sessionLog = Find-SessionLog -SandboxRoot $SandboxRoot
        Assert-Path -Path $sessionLog -Description "$Label session log"

        $logText = Get-Content -Path $sessionLog -Raw
        if ($logText -notmatch "opened 1 restored Windows workspace host window\(s\)") {
            throw "$Label did not restore a saved workspace session.\n$logText"
        }
        if ($logText -notmatch "web pane \d+ navigating to https://example.com") {
            throw "$Label did not restore the web tile.\n$logText"
        }
    }
    finally {
        $env:APPDATA = $previousEnvironment.APPDATA
        $env:LOCALAPPDATA = $previousEnvironment.LOCALAPPDATA
        $env:USERPROFILE = $previousEnvironment.USERPROFILE
        $env:HOME = $previousEnvironment.HOME
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
Invoke-LaunchSmoke -ExePath $PortableExe -SandboxRoot (Join-Path $SmokeRoot "portable-profile") -Label "Portable build"

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
Invoke-LaunchSmoke -ExePath $InstalledExe -SandboxRoot (Join-Path $SmokeRoot "installed-profile") -Label "Installed build"

Write-Host "Windows smoke test passed"
