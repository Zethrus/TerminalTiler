param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$SkipBuild,
    [switch]$SkipLaunchSmoke,
    [ValidateSet("clean-first-run", "mixed", "terminal-only")]
    [string]$SmokeProfileKind = "terminal-only"
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

    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path $Path)) {
        throw "$Description was not found at $Path"
    }
}

function Convert-ToTomlPath {
    param([string]$Path)

    return ($Path -replace '\\', '\\\\')
}

function Initialize-SmokeProfile {
    param(
        [string]$SandboxRoot,
        [string]$ProfileKind
    )

    $workspaceRoot = Join-Path $SandboxRoot "workspace"
    $profileRoot = Join-Path $SandboxRoot "profile"
    $configRoot = Join-Path $profileRoot "config"
    $dataRoot = Join-Path $profileRoot "data"
    $localDataRoot = Join-Path $profileRoot "local-data"
    $logsRoot = Join-Path $profileRoot "state\logs"

    New-Item -ItemType Directory -Force -Path (Join-Path $workspaceRoot "src") | Out-Null
    foreach ($dir in @($configRoot, $dataRoot, $localDataRoot, $logsRoot)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }

    $restoreMode = if ($ProfileKind -eq "clean-first-run") { "prompt" } else { "shell-only" }
    $preferences = @"
version = 1
default_restore_mode = "$restoreMode"
"@
    Set-Content -Path (Join-Path $configRoot "preferences.toml") -Value $preferences -Encoding ASCII

    $profile = @{
        ProfileRoot = $profileRoot
        AppData = Join-Path $SandboxRoot "AppData\Roaming"
        LocalAppData = Join-Path $SandboxRoot "AppData\Local"
        UserProfile = Join-Path $SandboxRoot "User"
    }

    if ($ProfileKind -eq "clean-first-run") {
        return $profile
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
    Set-Content -Path (Join-Path $dataRoot "session.toml") -Value $session -Encoding ASCII

    return $profile
}

function Find-SessionLog {
    param([string]$SandboxRoot)

    return Get-ChildItem -Path $SandboxRoot -Filter "terminaltiler-session.log" -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
}

function Find-SmokeLogs {
    param([string]$SandboxRoot)

    return Get-ChildItem -Path $SandboxRoot -Include "terminaltiler-session.log", "terminaltiler.log" -Recurse -ErrorAction SilentlyContinue
}

function Format-ExitCode {
    param([int]$ExitCode)

    $unsignedExitCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int32]$ExitCode), 0)
    return "$ExitCode (0x$($unsignedExitCode.ToString('X8')))"
}

function Write-ApplicationEventLogDiagnostics {
    param([string]$Label)

    Write-Host "--- Windows Application Event Log: $Label ---"
    try {
        $events = Get-WinEvent -FilterHashtable @{
            LogName = "Application"
            StartTime = (Get-Date).AddMinutes(-10)
        } -ErrorAction Stop |
            Where-Object {
                $_.ProviderName -like "*TerminalTiler*" -or
                $_.Message -like "*TerminalTiler*.exe*" -or
                $_.Message -like "*TerminalTiler.exe*"
            } |
            Select-Object -First 20 TimeCreated, Id, ProviderName, LevelDisplayName, Message

        if (-not $events) {
            Write-Host "No recent TerminalTiler Application Event Log entries were found."
            return
        }

        $events | Format-List | Out-String | Write-Host
    }
    catch {
        Write-Host "Could not read Windows Application Event Log: $_"
    }
}

function Write-SmokeDiagnostics {
    param(
        [string]$SandboxRoot,
        [string]$Label,
        [object]$ExitCode
    )

    Write-Host "==> diagnostics for $Label"
    if ($null -ne $ExitCode) {
        Write-Host "Process exit code: $(Format-ExitCode -ExitCode $ExitCode)"
    }
    $logs = @(Find-SmokeLogs -SandboxRoot $SandboxRoot)
    if ($logs.Count -eq 0) {
        Write-Host "No TerminalTiler logs were found under $SandboxRoot"
    }
    else {
        foreach ($log in $logs) {
            Write-Host "--- $($log.FullName) ---"
            Get-Content -Path $log.FullName -Raw -ErrorAction SilentlyContinue | Write-Host
        }
    }
    Write-ApplicationEventLogDiagnostics -Label $Label
}

function Wait-ForMainWindow {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$TimeoutSeconds = 8
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) {
            return $false
        }
        if ($Process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $true
        }
        Start-Sleep -Milliseconds 250
    }
    $Process.Refresh()
    return (-not $Process.HasExited -and $Process.MainWindowHandle -ne [IntPtr]::Zero)
}

function Wait-ForSessionLogPattern {
    param(
        [string]$SandboxRoot,
        [System.Diagnostics.Process]$Process,
        [string]$Pattern,
        [int]$TimeoutSeconds = 20
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    $latestText = ""
    while ((Get-Date) -lt $deadline) {
        $sessionLog = Find-SessionLog -SandboxRoot $SandboxRoot
        if (-not [string]::IsNullOrWhiteSpace($sessionLog) -and (Test-Path $sessionLog)) {
            $latestText = Get-Content -Path $sessionLog -Raw
            if ($latestText -match $Pattern) {
                return $latestText
            }
        }

        $Process.Refresh()
        if ($Process.HasExited -and $Process.ExitCode -ne 0) {
            throw "Process exited while waiting for smoke log pattern '$Pattern' with code $(Format-ExitCode -ExitCode $Process.ExitCode).`n$latestText"
        }

        Start-Sleep -Milliseconds 250
    }

    throw "Timed out waiting for smoke log pattern '$Pattern'.`n$latestText"
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
        TERMINALTILER_PROFILE_ROOT = $env:TERMINALTILER_PROFILE_ROOT
    }
    $process = $null

    New-Item -ItemType Directory -Force -Path $profile.UserProfile | Out-Null
    New-Item -ItemType Directory -Force -Path $profile.AppData | Out-Null
    New-Item -ItemType Directory -Force -Path $profile.LocalAppData | Out-Null
    $env:APPDATA = $profile.AppData
    $env:LOCALAPPDATA = $profile.LocalAppData
    $env:USERPROFILE = $profile.UserProfile
    $env:HOME = $profile.UserProfile
    $env:TERMINALTILER_PROFILE_ROOT = $profile.ProfileRoot

    try {
        $process = Start-Process -FilePath $ExePath -PassThru
        $hasMainWindow = Wait-ForMainWindow -Process $process -TimeoutSeconds 8
        Start-Sleep -Seconds 2
        $process.Refresh()
        if ($process.HasExited -and $process.ExitCode -ne 0) {
            throw "Process $ExePath exited with code $(Format-ExitCode -ExitCode $process.ExitCode)"
        }
        if (-not $hasMainWindow) {
            throw "$Label did not create a visible launcher/workspace window before the smoke timeout."
        }

        $requiredPattern = if ($ProfileKind -eq "clean-first-run") {
            "Windows startup init complete"
        } elseif ($ProfileKind -eq "mixed") {
            "web pane \d+ navigating to https://example.com"
        } else {
            "opened 1 restored Windows workspace host window\(s\)"
        }
        $logText = Wait-ForSessionLogPattern -SandboxRoot $SandboxRoot -Process $process -Pattern $requiredPattern -TimeoutSeconds 20

        if (-not $process.HasExited) {
            Stop-Process -Id $process.Id -Force
        }

        if ($ProfileKind -eq "clean-first-run") {
            if ($logText -notmatch "windows GUI shell startup" -or $logText -notmatch "Windows launcher window created" -or $logText -notmatch "Windows startup init complete") {
                throw "$Label did not complete launcher initialization.`n$logText"
            }
            if ($logText -match "opened \d+ restored Windows workspace host window") {
                throw "$Label unexpectedly restored a workspace during clean first-run.`n$logText"
            }
        } else {
            if ($logText -notmatch "opened 1 restored Windows workspace host window\(s\)") {
                throw "$Label did not restore a saved workspace session.`n$logText"
            }
            if ($ProfileKind -eq "mixed" -and $logText -notmatch "web pane \d+ navigating to https://example.com") {
                throw "$Label did not restore the web tile.`n$logText"
            }
        }
    }
    catch {
        $exitCode = $null
        if ($process -and $process.HasExited) {
            $exitCode = $process.ExitCode
        }
        Write-SmokeDiagnostics -SandboxRoot $SandboxRoot -Label $Label -ExitCode $exitCode
        throw
    }
    finally {
        if ($process -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
        $env:APPDATA = $previousEnvironment.APPDATA
        $env:LOCALAPPDATA = $previousEnvironment.LOCALAPPDATA
        $env:USERPROFILE = $previousEnvironment.USERPROFILE
        $env:HOME = $previousEnvironment.HOME
        $env:TERMINALTILER_PROFILE_ROOT = $previousEnvironment.TERMINALTILER_PROFILE_ROOT
    }
}

function Invoke-OptionalLaunchSmoke {
    param(
        [string]$ExePath,
        [string]$SandboxRoot,
        [string]$Label,
        [string]$ProfileKind,
        [bool]$SkipLaunchSmoke,
        [string]$SkipMessage
    )

    if ($SkipLaunchSmoke) {
        Write-Host "==> $SkipMessage"
        return
    }

    Write-Host "==> smoke-launching $Label"
    Invoke-LaunchSmoke -ExePath $ExePath -SandboxRoot $SandboxRoot -Label $Label -ProfileKind $ProfileKind
}

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DistDir = Join-Path $RootDir "dist"
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

Invoke-OptionalLaunchSmoke `
    -ExePath $PortableExePath `
    -SandboxRoot (Join-Path $SmokeRoot "portable-direct-clean-profile") `
    -Label "Portable executable clean first-run" `
    -ProfileKind "clean-first-run" `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping direct portable executable clean first-run launch smoke"

Invoke-OptionalLaunchSmoke `
    -ExePath $PortableExePath `
    -SandboxRoot (Join-Path $SmokeRoot "portable-direct-profile") `
    -Label "Portable executable restored terminal-only" `
    -ProfileKind "terminal-only" `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping direct portable executable restored-session launch smoke"

Write-Host "==> extracting portable zip"
New-Item -ItemType Directory -Force -Path $PortableExtractRoot | Out-Null
Expand-Archive -Path $ZipPath -DestinationPath $PortableExtractRoot -Force

$PortableExe = Join-Path $PortableExtractRoot "TerminalTiler.exe"
$PortableReadme = Join-Path $PortableExtractRoot "README-windows.txt"
Assert-Path -Path $PortableExe -Description "Portable executable"
Assert-Path -Path $PortableReadme -Description "Portable README"

Invoke-OptionalLaunchSmoke `
    -ExePath $PortableExe `
    -SandboxRoot (Join-Path $SmokeRoot "portable-profile") `
    -Label "Portable build" `
    -ProfileKind $SmokeProfileKind `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping portable executable launch smoke"

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

Invoke-OptionalLaunchSmoke `
    -ExePath $InstalledExe `
    -SandboxRoot (Join-Path $SmokeRoot "installed-profile") `
    -Label "Installed build" `
    -ProfileKind $SmokeProfileKind `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping NSIS-installed executable launch smoke"

Write-Host "==> smoke-installing MSI package"
Remove-Item -Recurse -Force $MsiInstallRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $MsiInstallRoot | Out-Null
$MsiInstallProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/i", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$MsiInstallRoot") -PassThru -Wait
if ($MsiInstallProcess.ExitCode -ne 0) {
    throw "MSI installer exited with code $($MsiInstallProcess.ExitCode)"
}

$MsiInstalledExe = Join-Path $MsiInstallRoot "TerminalTiler.exe"
Assert-Path -Path $MsiInstalledExe -Description "MSI-installed executable"

Invoke-OptionalLaunchSmoke `
    -ExePath $MsiInstalledExe `
    -SandboxRoot (Join-Path $SmokeRoot "msi-profile") `
    -Label "MSI build" `
    -ProfileKind $SmokeProfileKind `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping MSI-installed executable launch smoke"

Write-Host "==> smoke-uninstalling MSI package"
$MsiUninstallProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$MsiInstallRoot") -PassThru -Wait
if ($MsiUninstallProcess.ExitCode -ne 0) {
    throw "MSI uninstall exited with code $($MsiUninstallProcess.ExitCode)"
}

if (Test-Path $MsiInstalledExe) {
    throw "MSI uninstall left $MsiInstalledExe behind"
}

Write-Host "Windows smoke test passed"
