param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$SkipBuild,
    [switch]$SkipLaunchSmoke,
    [ValidateSet("mixed", "terminal-only")]
    [string]$SmokeProfileKind = "mixed"
)

$ErrorActionPreference = "Stop"

function Get-PackageVersion {
    param([string]$RootDir)

    if ($PackageVersion) {
        return $PackageVersion
    }

    $lastSuccessfulVersionFile = Join-Path $RootDir "packaging\.build\versioning\last-successful-version"
    if (Test-Path $lastSuccessfulVersionFile) {
        $lastSuccessfulVersion = (Get-Content -Path $lastSuccessfulVersionFile -Raw).Trim()
        if ($lastSuccessfulVersion) {
            return $lastSuccessfulVersion
        }
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

function Join-SmokePaths {
    param(
        [string]$Root,
        [string[]]$RelativePaths
    )

    return $RelativePaths | ForEach-Object { Join-Path $Root $_ }
}

function Initialize-SmokeProfile {
    param(
        [string]$SandboxRoot,
        [string]$ProfileKind
    )

    $workspaceRoot = Join-Path $SandboxRoot "workspace"
    $roamingRoot = Join-Path $SandboxRoot "AppData\Roaming"
    $localRoot = Join-Path $SandboxRoot "AppData\Local"
    $configTargets = Join-SmokePaths -Root $roamingRoot -RelativePaths @(
        "dev\Zethrus\TerminalTiler\config"
        "dev\Zethrus\TerminalTiler"
    )
    $dataTargets = Join-SmokePaths -Root $roamingRoot -RelativePaths @(
        "dev\Zethrus\TerminalTiler\data"
        "dev\Zethrus\TerminalTiler"
    )

    New-Item -ItemType Directory -Force -Path (Join-Path $workspaceRoot "src") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $localRoot "dev\Zethrus\TerminalTiler\state\logs") | Out-Null

    foreach ($dir in $configTargets + $dataTargets) {
        if ($dir -isnot [string]) {
            throw "Smoke profile target path must be a string, got '$($dir.GetType().FullName)'"
        }
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
    if ($ProfileKind -eq "terminal-only") {
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
kind = "tile"
id = "terminal-smoke"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.working_directory]
type = "workspace-root"
"@
    } else {
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
    }
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
    param(
        [string]$ExePath,
        [string]$SandboxRoot,
        [string]$Label,
        [string]$ProfileKind
    )

    $profile = Initialize-SmokeProfile -SandboxRoot $SandboxRoot -ProfileKind $ProfileKind
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
        if ($ProfileKind -eq "mixed" -and $logText -notmatch "web pane \d+ navigating to https://example.com") {
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
$DistDir = Join-Path $RootDir "dist"
$ResolvedVersion = $null
$PortableExePath = $null
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$MsiPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.msi"
$SmokeRoot = Join-Path $RootDir "packaging\.build\windows-smoke"
$PortableExtractRoot = Join-Path $SmokeRoot "portable"
$NsisInstallRoot = Join-Path $SmokeRoot "install-nsis"
$MsiInstallRoot = Join-Path $SmokeRoot "install-msi"

if (-not $SkipBuild) {
    $BuildScript = Join-Path $RootDir "packaging\build-windows.ps1"
    if ($PackageVersion) {
        & $BuildScript -PackageVersion $PackageVersion -RequireInstallers
    } else {
        & $BuildScript -RequireInstallers
    }
}

$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$PortableExePath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-portable-x86_64.exe"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$MsiPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.msi"

Remove-Item -Recurse -Force $SmokeRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $SmokeRoot | Out-Null

Write-Host "==> checking Windows release artifacts"
Assert-Path -Path $PortableExePath -Description "Portable executable"
Assert-Path -Path $ZipPath -Description "Portable zip"
Assert-Path -Path $InstallerPath -Description "Installer"
Assert-Path -Path $MsiPath -Description "MSI installer"

if (-not $SkipLaunchSmoke) {
    Write-Host "==> smoke-launching direct portable executable"
    Invoke-LaunchSmoke -ExePath $PortableExePath -SandboxRoot (Join-Path $SmokeRoot "portable-direct-profile") -Label "Portable executable" -ProfileKind $SmokeProfileKind
} else {
    Write-Host "==> skipping direct portable executable launch smoke"
}

Write-Host "==> extracting portable zip"
New-Item -ItemType Directory -Force -Path $PortableExtractRoot | Out-Null
Expand-Archive -Path $ZipPath -DestinationPath $PortableExtractRoot -Force

$PortableExe = Join-Path $PortableExtractRoot "TerminalTiler.exe"
$PortableReadme = Join-Path $PortableExtractRoot "README-windows.txt"
Assert-Path -Path $PortableExe -Description "Portable executable"
Assert-Path -Path $PortableReadme -Description "Portable README"

if (-not $SkipLaunchSmoke) {
    Write-Host "==> smoke-launching portable executable"
    Invoke-LaunchSmoke -ExePath $PortableExe -SandboxRoot (Join-Path $SmokeRoot "portable-profile") -Label "Portable build" -ProfileKind $SmokeProfileKind
} else {
    Write-Host "==> skipping portable executable launch smoke"
}

Write-Host "==> smoke-installing NSIS package"
New-Item -ItemType Directory -Force -Path $NsisInstallRoot | Out-Null
$InstallerArgs = @("/S", "/D=$NsisInstallRoot")
$InstallerProcess = Start-Process -FilePath $InstallerPath -ArgumentList $InstallerArgs -PassThru -Wait
if ($InstallerProcess.ExitCode -ne 0) {
    throw "Installer exited with code $($InstallerProcess.ExitCode)"
}

$InstalledExe = Join-Path $NsisInstallRoot "TerminalTiler.exe"
$InstalledUninstaller = Join-Path $NsisInstallRoot "Uninstall.exe"
Assert-Path -Path $InstalledExe -Description "Installed executable"
Assert-Path -Path $InstalledUninstaller -Description "Installed uninstaller"

if (-not $SkipLaunchSmoke) {
    Write-Host "==> smoke-launching installed executable"
    Invoke-LaunchSmoke -ExePath $InstalledExe -SandboxRoot (Join-Path $SmokeRoot "installed-profile") -Label "Installed build" -ProfileKind $SmokeProfileKind
} else {
    Write-Host "==> skipping NSIS-installed executable launch smoke"
}

Write-Host "==> smoke-installing MSI package"
Remove-Item -Recurse -Force $MsiInstallRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $MsiInstallRoot | Out-Null
$MsiInstallProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/i", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$MsiInstallRoot") -PassThru -Wait
if ($MsiInstallProcess.ExitCode -ne 0) {
    throw "MSI installer exited with code $($MsiInstallProcess.ExitCode)"
}

$MsiInstalledExe = Join-Path $MsiInstallRoot "TerminalTiler.exe"
Assert-Path -Path $MsiInstalledExe -Description "MSI-installed executable"

if (-not $SkipLaunchSmoke) {
    Write-Host "==> smoke-launching MSI-installed executable"
    Invoke-LaunchSmoke -ExePath $MsiInstalledExe -SandboxRoot (Join-Path $SmokeRoot "msi-profile") -Label "MSI build" -ProfileKind $SmokeProfileKind
} else {
    Write-Host "==> skipping MSI-installed executable launch smoke"
}

Write-Host "==> smoke-uninstalling MSI package"
$MsiUninstallProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", $MsiPath, "/qn", "/norestart") -PassThru -Wait
if ($MsiUninstallProcess.ExitCode -ne 0) {
    throw "MSI uninstall exited with code $($MsiUninstallProcess.ExitCode)"
}

if (Test-Path $MsiInstalledExe) {
    throw "MSI uninstall left $MsiInstalledExe behind"
}

Write-Host "Windows smoke test passed"
