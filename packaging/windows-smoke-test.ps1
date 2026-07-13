param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$UseGtkShell,
    [switch]$UseWin32Shell,
    [string]$GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT,
    [switch]$SkipBuild,
    [switch]$SkipLaunchSmoke,
    [ValidateSet("clean-first-run", "mixed", "terminal-only")]
    [string]$SmokeProfileKind = "terminal-only"
)

$ErrorActionPreference = "Stop"
$GtkMixedWebView2SmokeTimeoutSeconds = 75

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

function Assert-DirectoryHasFiles {
    param([string]$Path, [string]$Description)

    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path $Path -PathType Container)) {
        throw "$Description was not found at $Path"
    }
    if (-not (Get-ChildItem -Path $Path -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw "$Description at $Path did not contain any files"
    }
}

function Assert-WindowsGtkPayload {
    param(
        [string]$PayloadRoot,
        [switch]$RequireMarker
    )

    Assert-Path -Path (Join-Path $PayloadRoot "share\style.css") -Description "Shared GTK CSS"
    Assert-Path -Path (Join-Path $PayloadRoot "share\terminaltiler.svg") -Description "TerminalTiler GTK logo"
    Assert-Path -Path (Join-Path $PayloadRoot "share\icons\hicolor\scalable\apps\terminaltiler.svg") -Description "TerminalTiler GTK icon theme logo"
    Assert-Path -Path (Join-Path $PayloadRoot "share\terminaltiler.ico") -Description "TerminalTiler Windows icon"
    Assert-Path -Path (Join-Path $PayloadRoot "share\hover-icons\terminal.svg") -Description "GTK terminal hover icon"
    Assert-Path -Path (Join-Path $PayloadRoot "share\hover-icons\layout-dashboard.svg") -Description "GTK dashboard hover icon"
    Assert-Path -Path (Join-Path $PayloadRoot "share\hover-icons\save.svg") -Description "GTK save hover icon"
    Assert-Path -Path (Join-Path $PayloadRoot "terminaltiler-updater.exe") -Description "Updater helper"
    if ($RequireMarker) {
        Assert-Path -Path (Join-Path $PayloadRoot "terminaltiler-install-kind") -Description "Portable installer marker"
    }
}

function Assert-EmbeddedPackageVersion {
    param(
        [string]$ExePath,
        [string]$ExpectedVersion
    )

    $capabilities = & $ExePath --runtime-capabilities 2>$null
    $capabilitiesText = $capabilities -join "`n"
    if (-not ($capabilitiesText -match ('"core_package_version":"' + [regex]::Escape($ExpectedVersion) + '"'))) {
        throw "Packaged runtime at $ExePath did not report PACKAGE_VERSION $ExpectedVersion"
    }
}

function Assert-WindowsGtkRuntimePayload {
    param([string]$PayloadRoot)

    if (-not (Get-ChildItem -Path $PayloadRoot -Filter "*gtk-4*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw "GTK4 runtime DLL was not found in $PayloadRoot"
    }
    if (-not (Get-ChildItem -Path $PayloadRoot -Filter "*adwaita*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1)) {
        throw "libadwaita runtime DLL was not found in $PayloadRoot"
    }
    foreach ($relative in @(
        "etc",
        "lib\gdk-pixbuf-2.0",
        "share\glib-2.0",
        "share\icons"
    )) {
        Assert-DirectoryHasFiles -Path (Join-Path $PayloadRoot $relative) -Description "GTK runtime resource $relative"
    }
    Assert-Path -Path (Join-Path $PayloadRoot "lib\gio") -Description "GTK runtime resource lib\gio"
    Assert-Path -Path (Join-Path $PayloadRoot "lib\gtk-4.0") -Description "GTK runtime resource lib\gtk-4.0"
    Assert-Path -Path (Join-Path $PayloadRoot "share\themes") -Description "GTK runtime resource share\themes"
}

function Assert-NsisIconMetadata {
    param([string]$InstallRoot)

    $iconPath = Join-Path $InstallRoot "share\terminaltiler.ico"
    Assert-Path -Path $iconPath -Description "Installed TerminalTiler Windows icon"

    $programsPath = [Environment]::GetFolderPath("Programs")
    if ([string]::IsNullOrWhiteSpace($programsPath)) {
        throw "Could not resolve current-user Start Menu Programs folder"
    }

    $shell = $null
    try {
        $shell = New-Object -ComObject WScript.Shell
        foreach ($shortcut in @(
                @{
                    Path = Join-Path $programsPath "TerminalTiler\TerminalTiler.lnk"
                    Description = "TerminalTiler Start Menu shortcut"
                },
                @{
                    Path = Join-Path $programsPath "TerminalTiler\Uninstall TerminalTiler.lnk"
                    Description = "TerminalTiler uninstaller Start Menu shortcut"
                }
            )) {
            $shortcutPath = $shortcut["Path"]
            $shortcutDescription = $shortcut["Description"]
            Assert-Path -Path $shortcutPath -Description $shortcutDescription
            $shortcutMetadata = $shell.CreateShortcut($shortcutPath)
            $shortcutIcon = ($shortcutMetadata.IconLocation -replace ',\d+$', '')
            if ([string]::IsNullOrWhiteSpace($shortcutIcon)) {
                throw "$shortcutDescription did not define IconLocation"
            }
            if ([System.IO.Path]::GetFullPath($shortcutIcon) -ne [System.IO.Path]::GetFullPath($iconPath)) {
                throw "$shortcutDescription IconLocation '$($shortcutMetadata.IconLocation)' did not point at $iconPath"
            }
        }
    }
    finally {
        if ($shell) {
            [System.Runtime.InteropServices.Marshal]::ReleaseComObject($shell) | Out-Null
        }
    }

    $uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler"
    if (-not (Test-Path $uninstallKey)) {
        throw "TerminalTiler uninstall registry key was not found at $uninstallKey"
    }
    $displayIcon = (Get-ItemProperty -Path $uninstallKey -Name "DisplayIcon").DisplayIcon
    if ([System.IO.Path]::GetFullPath($displayIcon) -ne [System.IO.Path]::GetFullPath($iconPath)) {
        throw "TerminalTiler uninstall DisplayIcon '$displayIcon' did not point at $iconPath"
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
    $tempRoot = Join-Path $SandboxRoot "Temp"
    $tmpRoot = Join-Path $SandboxRoot "Tmp"

    New-Item -ItemType Directory -Force -Path (Join-Path $workspaceRoot "src") | Out-Null
    foreach ($dir in @($configRoot, $dataRoot, $localDataRoot, $logsRoot, $tempRoot, $tmpRoot)) {
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
        Temp = $tempRoot
        Tmp = $tmpRoot
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

function Get-SmokeSessionLogText {
    param([string]$SandboxRoot)

    $sessionLog = Find-SessionLog -SandboxRoot $SandboxRoot
    if ([string]::IsNullOrWhiteSpace($sessionLog) -or -not (Test-Path $sessionLog)) {
        return ""
    }

    return Get-Content -Path $sessionLog -Raw
}

function Format-ExitCode {
    param([int]$ExitCode)

    $unsignedExitCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int32]$ExitCode), 0)
    return "$ExitCode (0x$($unsignedExitCode.ToString('X8')))"
}

function Convert-ToSafeFileName {
    param([string]$Value)

    $safe = $Value -replace '[^A-Za-z0-9._-]+', '-'
    $safe = $safe.Trim('-')
    if ([string]::IsNullOrWhiteSpace($safe)) {
        return "smoke-diagnostics"
    }

    return $safe
}

function Get-TerminalTilerSmokeProcesses {
    return @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -like "TerminalTiler*.exe" -or
            $_.ExecutablePath -like "*\TerminalTiler*.exe"
        })
}

function Write-ApplicationEventLogDiagnostics {
    param(
        [string]$Label,
        [string]$ExePath = "",
        [datetime]$LaunchStartTime = (Get-Date).AddMinutes(-10),
        [string]$OutputPath = ""
    )

    Write-Host "--- Windows Application Event Log: $Label ---"
    try {
        $resolvedExePath = ""
        if (-not [string]::IsNullOrWhiteSpace($ExePath)) {
            try {
                $resolvedExePath = [System.IO.Path]::GetFullPath($ExePath)
            }
            catch {
                $resolvedExePath = $ExePath
            }
        }
        $exeName = if ([string]::IsNullOrWhiteSpace($resolvedExePath)) { "TerminalTiler.exe" } else { Split-Path -Leaf $resolvedExePath }

        $events = Get-WinEvent -FilterHashtable @{
            LogName = "Application"
            StartTime = $LaunchStartTime
        } -ErrorAction Stop |
            Where-Object {
                $message = [string]$_.Message
                $_.ProviderName -like "*TerminalTiler*" -or
                (-not [string]::IsNullOrWhiteSpace($resolvedExePath) -and $message -like "*$resolvedExePath*") -or
                $message -like "*$exeName*"
            } |
            Select-Object -First 20 TimeCreated, Id, ProviderName, LevelDisplayName, Message

        $output = ""
        if (-not $events) {
            $output = "No TerminalTiler Application Event Log entries were found for $exeName after $LaunchStartTime."
        }
        else {
            $output = $events | Format-List | Out-String
        }

        $output | Write-Host
        if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
            Set-Content -Path $OutputPath -Value $output -Encoding UTF8
        }
    }
    catch {
        $message = "Could not read Windows Application Event Log: $_"
        Write-Host $message
        if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
            Set-Content -Path $OutputPath -Value $message -Encoding UTF8
        }
    }
}

function Write-SmokeDiagnostics {
    param(
        [string]$SandboxRoot,
        [string]$Label,
        [object]$ExitCode,
        [string]$ExePath = "",
        [datetime]$LaunchStartTime = (Get-Date).AddMinutes(-10)
    )

    Write-Host "==> diagnostics for $Label"
    $safeLabel = Convert-ToSafeFileName -Value $Label
    $diagnosticRoot = $null
    if (-not [string]::IsNullOrWhiteSpace($script:DiagnosticsRoot)) {
        $diagnosticRoot = Join-Path $script:DiagnosticsRoot $safeLabel
        New-Item -ItemType Directory -Force -Path $diagnosticRoot | Out-Null
    }

    $summary = New-Object System.Collections.Generic.List[string]
    $summary.Add("Label: $Label")
    $summary.Add("Executable: $ExePath")
    $summary.Add("SandboxRoot: $SandboxRoot")
    $summary.Add("LaunchStartTime: $($LaunchStartTime.ToString('o'))")
    if ($null -ne $ExitCode) {
        $formattedExitCode = Format-ExitCode -ExitCode $ExitCode
        Write-Host "Process exit code: $formattedExitCode"
        $summary.Add("ProcessExitCode: $formattedExitCode")
    }
    else {
        $summary.Add("ProcessExitCode: <not available>")
    }
    $summary.Add("APPDATA: $env:APPDATA")
    $summary.Add("LOCALAPPDATA: $env:LOCALAPPDATA")
    $summary.Add("USERPROFILE: $env:USERPROFILE")
    $summary.Add("TEMP: $env:TEMP")
    $summary.Add("TMP: $env:TMP")
    $summary.Add("TERMINALTILER_PROFILE_ROOT: $env:TERMINALTILER_PROFILE_ROOT")

    $webView2UserDataFolder = Join-Path $SandboxRoot "profile\local-data\webview2"
    Write-Host "Resolved WebView2 user data folder: $webView2UserDataFolder"
    $summary.Add("WebView2UserDataFolder: $webView2UserDataFolder")
    if ($diagnosticRoot) {
        Set-Content -Path (Join-Path $diagnosticRoot "summary.txt") -Value $summary -Encoding UTF8
    }

    $processSnapshot = Get-TerminalTilerSmokeProcesses |
        Select-Object ProcessId, ParentProcessId, Name, ExecutablePath, CommandLine |
        Format-List |
        Out-String
    if ([string]::IsNullOrWhiteSpace($processSnapshot)) {
        $processSnapshot = "No TerminalTiler process snapshot entries were found."
    }
    Write-Host "--- TerminalTiler process snapshot ---"
    $processSnapshot | Write-Host
    if ($diagnosticRoot) {
        Set-Content -Path (Join-Path $diagnosticRoot "process-snapshot.txt") -Value $processSnapshot -Encoding UTF8
    }

    if (Test-Path $webView2UserDataFolder) {
        $webView2Listing = Get-ChildItem -Path $webView2UserDataFolder -Force -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 200 FullName, Length, LastWriteTime |
            Format-Table -AutoSize |
            Out-String
        $webView2Listing | Write-Host
        if ($diagnosticRoot) {
            Set-Content -Path (Join-Path $diagnosticRoot "webview2-tree.txt") -Value $webView2Listing -Encoding UTF8
        }
    }
    else {
        Write-Host "WebView2 user data folder was not created."
        if ($diagnosticRoot) {
            Set-Content -Path (Join-Path $diagnosticRoot "webview2-tree.txt") -Value "WebView2 user data folder was not created." -Encoding UTF8
        }
    }

    if (Test-Path $SandboxRoot) {
        $profileTree = Get-ChildItem -Path $SandboxRoot -Force -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 500 FullName, Length, LastWriteTime |
            Format-Table -AutoSize |
            Out-String
        if ($diagnosticRoot) {
            Set-Content -Path (Join-Path $diagnosticRoot "sandbox-tree.txt") -Value $profileTree -Encoding UTF8
        }
    }

    $logs = @(Find-SmokeLogs -SandboxRoot $SandboxRoot)
    if ($logs.Count -eq 0) {
        Write-Host "No TerminalTiler logs were found under $SandboxRoot"
    }
    else {
        $logsRoot = $null
        if ($diagnosticRoot) {
            $logsRoot = Join-Path $diagnosticRoot "logs"
            New-Item -ItemType Directory -Force -Path $logsRoot | Out-Null
        }
        foreach ($log in $logs) {
            Write-Host "--- $($log.FullName) ---"
            Get-Content -Path $log.FullName -Raw -ErrorAction SilentlyContinue | Write-Host
            if ($logsRoot) {
                $relativeLogPath = $log.FullName.Substring($SandboxRoot.Length)
                $relativeLogPath = $relativeLogPath.TrimStart([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
                $relativeName = Convert-ToSafeFileName -Value $relativeLogPath
                Copy-Item -Path $log.FullName -Destination (Join-Path $logsRoot $relativeName) -Force -ErrorAction SilentlyContinue
            }
        }
    }

    $eventLogPath = if ($diagnosticRoot) { Join-Path $diagnosticRoot "application-event-log.txt" } else { "" }
    Write-ApplicationEventLogDiagnostics -Label $Label -ExePath $ExePath -LaunchStartTime $LaunchStartTime -OutputPath $eventLogPath

    if ($diagnosticRoot) {
        Write-Host "Staged Windows smoke diagnostics at $diagnosticRoot"
    }
}

function Test-PreLogLaunchFailure {
    param(
        [string]$SandboxRoot,
        [object]$ExitCode
    )

    if ($null -eq $ExitCode) {
        return $false
    }
    if ((Find-SmokeLogs -SandboxRoot $SandboxRoot | Select-Object -First 1)) {
        return $false
    }

    $unsignedExitCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int32]$ExitCode), 0)
    return $unsignedExitCode -eq 0xC0000142
}

function Wait-ForMainWindow {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$TimeoutSeconds = 8
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $Process.Refresh()
        if (Test-ProcessTreeHasMainWindow -RootProcessId $Process.Id) {
            return $true
        }
        if ($Process.HasExited -and -not (Get-DescendantProcessIds -RootProcessId $Process.Id | Select-Object -First 1)) {
            return $false
        }
        Start-Sleep -Milliseconds 250
    }
    $Process.Refresh()
    return (Test-ProcessTreeHasMainWindow -RootProcessId $Process.Id)
}

function Get-DescendantProcessIds {
    param([int]$RootProcessId)

    $descendants = New-Object System.Collections.Generic.List[int]
    $pending = New-Object System.Collections.Generic.Queue[int]
    $pending.Enqueue($RootProcessId)

    while ($pending.Count -gt 0) {
        $parentId = $pending.Dequeue()
        try {
            $children = Get-CimInstance Win32_Process -Filter "ParentProcessId = $parentId" -ErrorAction Stop
        }
        catch {
            $children = @()
        }

        foreach ($child in $children) {
            $childId = [int]$child.ProcessId
            if (-not $descendants.Contains($childId)) {
                $descendants.Add($childId)
                $pending.Enqueue($childId)
            }
        }
    }

    return $descendants
}

function Test-ProcessTreeHasMainWindow {
    param([int]$RootProcessId)

    $processIds = @($RootProcessId) + @(Get-DescendantProcessIds -RootProcessId $RootProcessId)
    foreach ($processId in $processIds) {
        try {
            $candidate = Get-Process -Id $processId -ErrorAction Stop
            if ($candidate.MainWindowHandle -ne [IntPtr]::Zero) {
                return $true
            }
        }
        catch {
        }
    }

    return $false
}

function Stop-ProcessTree {
    param(
        [System.Diagnostics.Process]$Process,
        [int]$GracefulTimeoutSeconds = 5
    )

    $processIds = @(Get-DescendantProcessIds -RootProcessId $Process.Id)
    $allProcessIds = @($Process.Id) + $processIds

    foreach ($processId in $allProcessIds) {
        try {
            $candidate = Get-Process -Id $processId -ErrorAction Stop
            if ($candidate.MainWindowHandle -ne [IntPtr]::Zero) {
                $candidate.CloseMainWindow() | Out-Null
            }
        }
        catch {
        }
    }

    $deadline = (Get-Date).AddSeconds($GracefulTimeoutSeconds)
    do {
        $remaining = @($allProcessIds | Where-Object {
                try {
                    Get-Process -Id $_ -ErrorAction Stop | Out-Null
                    $true
                }
                catch {
                    $false
                }
            })
        if ($remaining.Count -eq 0) {
            $global:LASTEXITCODE = 0
            return
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)

    [array]::Reverse($processIds)
    foreach ($processId in $processIds) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }
    try {
        $Process.Refresh()
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    catch {
    }

    foreach ($processId in @($Process.Id) + $processIds) {
        try {
            $candidate = Get-Process -Id $processId -ErrorAction Stop
            $candidate.WaitForExit(5000) | Out-Null
        }
        catch {
        }
    }
    $global:LASTEXITCODE = 0
}

function Stop-TerminalTilerSmokeProcesses {
    param(
        [int]$TimeoutSeconds = 15,
        [switch]$ThrowOnTimeout
    )

    foreach ($candidate in (Get-TerminalTilerSmokeProcesses)) {
        Stop-Process -Id ([int]$candidate.ProcessId) -Force -ErrorAction SilentlyContinue
    }

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $remaining = @(Get-TerminalTilerSmokeProcesses)
        if ($remaining.Count -eq 0) {
            return
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)

    $remainingSummary = @(Get-TerminalTilerSmokeProcesses | Select-Object ProcessId, ParentProcessId, Name, ExecutablePath | Format-Table -AutoSize | Out-String) -join "`n"
    $message = "TerminalTiler smoke processes remained after cleanup timeout:`n$remainingSummary"
    if ($ThrowOnTimeout) {
        throw $message
    }
    Write-Warning $message
}

function Wait-ProcessOrTimeout {
    param(
        [System.Diagnostics.Process]$Process,
        [string]$Label,
        [int]$TimeoutSeconds = 180
    )

    if (-not $Process.WaitForExit($TimeoutSeconds * 1000)) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        throw "$Label timed out after $TimeoutSeconds seconds"
    }

    return $Process.ExitCode
}

function Test-TransientWindowsProcessStartFailure {
    param([object]$ExitCode)

    if ($null -eq $ExitCode) {
        return $false
    }

    $unsignedExitCode = [BitConverter]::ToUInt32([BitConverter]::GetBytes([int32]$ExitCode), 0)
    return $unsignedExitCode -eq 0xC0000142
}

function Invoke-ProcessWithRetry {
    param(
        [string]$FilePath,
        [string[]]$ArgumentList,
        [string]$Label,
        [int]$TimeoutSeconds = 180,
        [int]$MaxAttempts = 3,
        [scriptblock]$ShouldRetry = { param($ExitCode) $true },
        [scriptblock]$BeforeRetry = { }
    )

    $lastExitCode = $null
    for ($attempt = 1; $attempt -le $MaxAttempts; $attempt++) {
        $process = Start-Process -FilePath $FilePath -ArgumentList $ArgumentList -PassThru
        $lastExitCode = Wait-ProcessOrTimeout -Process $process -Label $Label -TimeoutSeconds $TimeoutSeconds
        if ($lastExitCode -eq 0) {
            return
        }

        if ($attempt -lt $MaxAttempts -and (& $ShouldRetry $lastExitCode)) {
            Write-Warning "$Label exited with code $(Format-ExitCode -ExitCode $lastExitCode) on attempt $attempt/$MaxAttempts; retrying after runner cleanup."
            & $BeforeRetry
            Stop-TerminalTilerSmokeProcesses
            Start-Sleep -Seconds (5 * $attempt)
            continue
        }

        break
    }

    throw "$Label exited with code $(Format-ExitCode -ExitCode $lastExitCode)"
}

function Invoke-MsiExecWithRetry {
    param(
        [string[]]$ArgumentList,
        [string]$Label,
        [int]$TimeoutSeconds = 180,
        [int]$MaxAttempts = 3
    )

    Invoke-ProcessWithRetry `
        -FilePath "msiexec.exe" `
        -ArgumentList $ArgumentList `
        -Label $Label `
        -TimeoutSeconds $TimeoutSeconds `
        -MaxAttempts $MaxAttempts
}

function Write-InstallerDiagnostics {
    param(
        [string]$Label,
        [string]$InstallerPath,
        [string]$InstallRoot,
        [object]$ExitCode,
        [datetime]$StartTime = (Get-Date).AddMinutes(-10)
    )

    Write-Host "==> diagnostics for $Label"
    $safeLabel = Convert-ToSafeFileName -Value $Label
    $diagnosticRoot = $null
    if (-not [string]::IsNullOrWhiteSpace($script:DiagnosticsRoot)) {
        $diagnosticRoot = Join-Path $script:DiagnosticsRoot $safeLabel
        New-Item -ItemType Directory -Force -Path $diagnosticRoot | Out-Null
    }

    $summary = New-Object System.Collections.Generic.List[string]
    $summary.Add("Label: $Label")
    $summary.Add("InstallerPath: $InstallerPath")
    $summary.Add("InstallRoot: $InstallRoot")
    $summary.Add("StartTime: $($StartTime.ToString('o'))")
    if ($null -ne $ExitCode) {
        $formattedExitCode = Format-ExitCode -ExitCode $ExitCode
        Write-Host "Installer exit code: $formattedExitCode"
        $summary.Add("InstallerExitCode: $formattedExitCode")
    }
    else {
        $summary.Add("InstallerExitCode: <not available>")
    }

    if ($diagnosticRoot) {
        Set-Content -Path (Join-Path $diagnosticRoot "summary.txt") -Value $summary -Encoding UTF8
    }

    if (Test-Path $InstallRoot) {
        $installTree = Get-ChildItem -Path $InstallRoot -Force -Recurse -ErrorAction SilentlyContinue |
            Select-Object -First 500 FullName, Length, LastWriteTime |
            Format-Table -AutoSize |
            Out-String
        if ($diagnosticRoot) {
            Set-Content -Path (Join-Path $diagnosticRoot "install-tree.txt") -Value $installTree -Encoding UTF8
        }
    }

    $eventLogPath = if ($diagnosticRoot) { Join-Path $diagnosticRoot "application-event-log.txt" } else { "" }
    Write-ApplicationEventLogDiagnostics -Label $Label -ExePath $InstallerPath -LaunchStartTime $StartTime -OutputPath $eventLogPath

    if ($diagnosticRoot) {
        Write-Host "Staged Windows installer diagnostics at $diagnosticRoot"
    }
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

function Get-LaunchSmokeRequiredPattern {
    param(
        [bool]$ExpectGtkShell,
        [string]$ProfileKind
    )

    if ($ExpectGtkShell) {
        if ($ProfileKind -eq "clean-first-run") {
            return "windows GTK shell loaded canonical GTK CSS"
        }
        if ($ProfileKind -eq "mixed") {
            return "Windows GTK WebView2 tile navigating to https://example.com"
        }
        return "Windows GTK shell restored interactive GTK workspace with"
    }

    if ($ProfileKind -eq "clean-first-run") {
        return "Windows startup init complete"
    }
    if ($ProfileKind -eq "mixed") {
        return "web pane \d+ navigating to https://example.com"
    }
    return "opened 1 restored Windows workspace host window\(s\)"
}

function Invoke-LaunchSmoke {
    param(
        [string]$ExePath,
        [string]$SandboxRoot,
        [string]$Label,
        [string]$ProfileKind,
        [string]$ExpectedLaunchRoot = ""
    )

    $expectGtkShell = -not $UseWin32Shell
    $maxLaunchAttempts = 2

    for ($attempt = 1; $attempt -le $maxLaunchAttempts; $attempt++) {
        if ($attempt -gt 1) {
            Write-Warning "$Label hit a pre-log Windows launch initialization failure; retrying once after isolated cleanup."
            Remove-Item -Recurse -Force $SandboxRoot -ErrorAction SilentlyContinue
        }

        Stop-TerminalTilerSmokeProcesses -ThrowOnTimeout
        $profile = Initialize-SmokeProfile -SandboxRoot $SandboxRoot -ProfileKind $ProfileKind
        $previousEnvironment = @{
            APPDATA = $env:APPDATA
            LOCALAPPDATA = $env:LOCALAPPDATA
            USERPROFILE = $env:USERPROFILE
            HOME = $env:HOME
            TEMP = $env:TEMP
            TMP = $env:TMP
            TERMINALTILER_PROFILE_ROOT = $env:TERMINALTILER_PROFILE_ROOT
            TERMINALTILER_DISABLE_UPDATES = $env:TERMINALTILER_DISABLE_UPDATES
        }
        $process = $null
        $launchStartTime = Get-Date

        New-Item -ItemType Directory -Force -Path $profile.UserProfile | Out-Null
        New-Item -ItemType Directory -Force -Path $profile.AppData | Out-Null
        New-Item -ItemType Directory -Force -Path $profile.LocalAppData | Out-Null
        New-Item -ItemType Directory -Force -Path $profile.Temp | Out-Null
        New-Item -ItemType Directory -Force -Path $profile.Tmp | Out-Null
        $env:APPDATA = $profile.AppData
        $env:LOCALAPPDATA = $profile.LocalAppData
        $env:USERPROFILE = $profile.UserProfile
        $env:HOME = $profile.UserProfile
        $env:TEMP = $profile.Temp
        $env:TMP = $profile.Tmp
        $env:TERMINALTILER_PROFILE_ROOT = $profile.ProfileRoot
        # Smoke tests validate packaged startup and restore behavior.  Keep
        # them hermetic: updater unit tests cover release discovery, while a
        # smoke run must never depend on GitHub availability or present a live
        # release dialog over the UI under test.
        $env:TERMINALTILER_DISABLE_UPDATES = "1"

        try {
            $webView2UserDataFolder = Join-Path $profile.ProfileRoot "local-data\webview2"
            Write-Host "$Label WebView2 user data folder: $webView2UserDataFolder"
            $process = Start-Process -FilePath $ExePath -PassThru
            $mainWindowTimeoutSeconds = if ($expectGtkShell) { 20 } else { 8 }
            $hasMainWindow = Wait-ForMainWindow -Process $process -TimeoutSeconds $mainWindowTimeoutSeconds
            Start-Sleep -Seconds 2
            $process.Refresh()
            if ($process.HasExited -and $process.ExitCode -ne 0) {
                throw "Process $ExePath exited with code $(Format-ExitCode -ExitCode $process.ExitCode)"
            }

            $requiredPattern = Get-LaunchSmokeRequiredPattern -ExpectGtkShell $expectGtkShell -ProfileKind $ProfileKind

            if (-not $hasMainWindow) {
                if (-not $expectGtkShell) {
                    throw "$Label did not create a visible launcher/workspace window before the smoke timeout."
                }

                Write-Host "$Label did not expose a Win32 MainWindowHandle before the smoke timeout; continuing with GTK session-log validation."
            }

            $logTimeoutSeconds = if ($expectGtkShell -and $ProfileKind -eq "mixed") { $GtkMixedWebView2SmokeTimeoutSeconds } else { 20 }
            $logText = Wait-ForSessionLogPattern -SandboxRoot $SandboxRoot -Process $process -Pattern $requiredPattern -TimeoutSeconds $logTimeoutSeconds

            Stop-ProcessTree -Process $process
            Stop-TerminalTilerSmokeProcesses -ThrowOnTimeout
            $finalLogText = Get-SmokeSessionLogText -SandboxRoot $SandboxRoot
            if (-not [string]::IsNullOrWhiteSpace($finalLogText)) {
                $logText = $finalLogText
            }

            if ($expectGtkShell) {
                if ($logText -notmatch "windows GTK shell startup" -or $logText -notmatch "windows GTK shell loaded canonical GTK CSS") {
                    throw "$Label did not complete GTK launcher initialization.`n$logText"
                }
                if ($ProfileKind -eq "clean-first-run" -and $logText -notmatch "GTK launch deck default workspace root resolved to") {
                    throw "$Label did not log the GTK launch deck default workspace root.`n$logText"
                }
                if (-not [string]::IsNullOrWhiteSpace($ExpectedLaunchRoot)) {
                    $expectedRoot = $ExpectedLaunchRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
                    $expectedRootPattern = [regex]::Escape($expectedRoot)
                    if ($logText -notmatch $expectedRootPattern) {
                        throw "$Label did not launch from the expected stable wrapper directory '$ExpectedLaunchRoot'.`n$logText"
                    }
                    if ($logText -match '\\nsx[0-9A-Fa-f]+\.tmp' -or $logText -match '\\AppData\\Local\\Temp\\nsx') {
                        throw "$Label launched from a temporary NSIS extraction directory instead of a stable workspace root.`n$logText"
                    }
                }
                if ($ProfileKind -ne "clean-first-run" -and $logText -notmatch "Windows GTK shell restored interactive GTK workspace with") {
                    throw "$Label did not restore inside the shared interactive GTK workspace.`n$logText"
                }
                if ($ProfileKind -eq "mixed" -and $logText -notmatch "Windows GTK WebView2 tile navigating to https://example.com") {
                    throw "$Label did not initialize the restored WebView2 browser tile.`n$logText"
                }
                if ($logText -match "opened \d+ restored Windows workspace host window") {
                    throw "$Label unexpectedly opened the legacy Win32 workspace host from the GTK parity shell.`n$logText"
                }
            }
            elseif ($ProfileKind -eq "clean-first-run") {
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

            return
        }
        catch {
            $exitCode = $null
            if ($process -and $process.HasExited) {
                $exitCode = $process.ExitCode
            }
            Write-SmokeDiagnostics -SandboxRoot $SandboxRoot -Label $Label -ExitCode $exitCode -ExePath $ExePath -LaunchStartTime $launchStartTime
            $shouldRetry = $attempt -lt $maxLaunchAttempts -and (Test-PreLogLaunchFailure -SandboxRoot $SandboxRoot -ExitCode $exitCode)
            if ($shouldRetry) {
                if ($process) {
                    Stop-ProcessTree -Process $process
                }
                Stop-TerminalTilerSmokeProcesses
                continue
            }
            throw
        }
        finally {
            if ($process) {
                Stop-ProcessTree -Process $process
                Stop-TerminalTilerSmokeProcesses
            }
            $env:APPDATA = $previousEnvironment.APPDATA
            $env:LOCALAPPDATA = $previousEnvironment.LOCALAPPDATA
            $env:USERPROFILE = $previousEnvironment.USERPROFILE
            $env:HOME = $previousEnvironment.HOME
            $env:TEMP = $previousEnvironment.TEMP
            $env:TMP = $previousEnvironment.TMP
            $env:TERMINALTILER_PROFILE_ROOT = $previousEnvironment.TERMINALTILER_PROFILE_ROOT
            $env:TERMINALTILER_DISABLE_UPDATES = $previousEnvironment.TERMINALTILER_DISABLE_UPDATES
        }
    }
}

function Invoke-OptionalLaunchSmoke {
    param(
        [string]$ExePath,
        [string]$SandboxRoot,
        [string]$Label,
        [string]$ProfileKind,
        [string]$ExpectedLaunchRoot = "",
        [bool]$SkipLaunchSmoke,
        [string]$SkipMessage
    )

    if ($SkipLaunchSmoke) {
        Write-Host "==> $SkipMessage"
        return
    }

    Write-Host "==> smoke-launching $Label"
    Invoke-LaunchSmoke -ExePath $ExePath -SandboxRoot $SandboxRoot -Label $Label -ProfileKind $ProfileKind -ExpectedLaunchRoot $ExpectedLaunchRoot
}

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DistDir = Join-Path $RootDir "dist"
$SmokeRoot = Join-Path $RootDir "packaging\.build\windows-smoke"
$DiagnosticsRoot = Join-Path $RootDir "artifacts\windows-smoke-diagnostics"
$script:DiagnosticsRoot = $DiagnosticsRoot
$PortableExtractRoot = Join-Path $SmokeRoot "portable"
$NsisInstallRoot = Join-Path $SmokeRoot "install-nsis"
$MsiInstallRoot = Join-Path $SmokeRoot "install-msi"

if ($UseGtkShell -and $UseWin32Shell) {
    throw "Use either the canonical GTK shell or the explicit Win32 fallback, not both."
}
$ExpectGtkShell = -not $UseWin32Shell

if (-not $SkipBuild) {
    $BuildScript = Join-Path $RootDir "packaging\build-windows.ps1"
    $BuildArgs = @{}
    if ($ExpectGtkShell) {
        & (Join-Path $RootDir "packaging\setup-windows-gtk.ps1") -GtkRuntimeRoot $GtkRuntimeRoot
        $BuildArgs.GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT
    }
    else {
        $BuildArgs.UseWin32Shell = $true
    }
    if ($PackageVersion) {
        $BuildArgs.PackageVersion = $PackageVersion
    }
    $BuildArgs.RequireInstallers = $true
    & $BuildScript @BuildArgs
}
elseif ($ExpectGtkShell -and $GtkRuntimeRoot) {
    & (Join-Path $RootDir "packaging\setup-windows-gtk.ps1") -GtkRuntimeRoot $GtkRuntimeRoot
}

$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$PortableExePath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-portable-x86_64.exe"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$MsiPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.msi"

Remove-Item -Recurse -Force $SmokeRoot -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $DiagnosticsRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $SmokeRoot | Out-Null
New-Item -ItemType Directory -Force -Path $DiagnosticsRoot | Out-Null

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
    -ExpectedLaunchRoot (Split-Path -Parent (Resolve-Path $PortableExePath).Path) `
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
if (Test-Path (Join-Path $PortableExtractRoot "terminaltiler-install-kind")) { throw "Portable ZIP unexpectedly contains an update provenance marker" }
Assert-WindowsGtkPayload -PayloadRoot $PortableExtractRoot
Assert-EmbeddedPackageVersion -ExePath $PortableExe -ExpectedVersion $ResolvedVersion
if ($ExpectGtkShell) {
    Assert-WindowsGtkRuntimePayload -PayloadRoot $PortableExtractRoot
}

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
$InstallerStartTime = Get-Date
try {
    Invoke-ProcessWithRetry `
        -FilePath $InstallerPath `
        -ArgumentList $InstallerArgs `
        -Label "NSIS installer" `
        -MaxAttempts 3 `
        -ShouldRetry { param($ExitCode) Test-TransientWindowsProcessStartFailure -ExitCode $ExitCode } `
        -BeforeRetry {
            Remove-Item -Recurse -Force $NsisInstallRoot -ErrorAction SilentlyContinue
            New-Item -ItemType Directory -Force -Path $NsisInstallRoot | Out-Null
        }
}
catch {
    $exitCode = $null
    if ($_ -match 'code (-?\d+)') {
        $exitCode = [int]$Matches[1]
    }
    Write-InstallerDiagnostics `
        -Label "NSIS installer" `
        -InstallerPath $InstallerPath `
        -InstallRoot $NsisInstallRoot `
        -ExitCode $exitCode `
        -StartTime $InstallerStartTime
    throw
}

$InstalledExe = Join-Path $NsisInstallRoot "TerminalTiler.exe"
$InstalledUninstaller = Join-Path $NsisInstallRoot "Uninstall.exe"
Assert-Path -Path $InstalledExe -Description "Installed executable"
Assert-Path -Path $InstalledUninstaller -Description "Installed uninstaller"
if ((Get-Content -Raw (Join-Path $NsisInstallRoot "terminaltiler-install-kind")).Trim() -ne "nsis") { throw "NSIS marker is incorrect" }
Assert-EmbeddedPackageVersion -ExePath $InstalledExe -ExpectedVersion $ResolvedVersion
Assert-WindowsGtkPayload -PayloadRoot $NsisInstallRoot
if ($ExpectGtkShell) {
    Assert-WindowsGtkRuntimePayload -PayloadRoot $NsisInstallRoot
}
Assert-NsisIconMetadata -InstallRoot $NsisInstallRoot

$NsisSmokeProfileKind = "mixed"
Invoke-OptionalLaunchSmoke `
    -ExePath $InstalledExe `
    -SandboxRoot (Join-Path $SmokeRoot "installed-profile") `
    -Label "Installed build" `
    -ProfileKind $NsisSmokeProfileKind `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping NSIS-installed executable launch smoke"

Write-Host "==> smoke-installing MSI package"
Remove-Item -Recurse -Force $MsiInstallRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $MsiInstallRoot | Out-Null
Invoke-MsiExecWithRetry -ArgumentList @("/i", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$MsiInstallRoot") -Label "MSI installer"

$MsiInstalledExe = Join-Path $MsiInstallRoot "TerminalTiler.exe"
Assert-Path -Path $MsiInstalledExe -Description "MSI-installed executable"
if ((Get-Content -Raw (Join-Path $MsiInstallRoot "terminaltiler-install-kind")).Trim() -ne "msi") { throw "MSI payload marker is incorrect" }
if ((Get-ItemProperty -Path "HKCU:\Software\Zethrus\TerminalTiler" -Name InstallerKind -ErrorAction Stop).InstallerKind -ne "msi") { throw "MSI marker is incorrect" }
Assert-EmbeddedPackageVersion -ExePath $MsiInstalledExe -ExpectedVersion $ResolvedVersion
Assert-WindowsGtkPayload -PayloadRoot $MsiInstallRoot
if ($ExpectGtkShell) {
    Assert-WindowsGtkRuntimePayload -PayloadRoot $MsiInstallRoot
}

Invoke-OptionalLaunchSmoke `
    -ExePath $MsiInstalledExe `
    -SandboxRoot (Join-Path $SmokeRoot "msi-profile") `
    -Label "MSI build" `
    -ProfileKind $SmokeProfileKind `
    -SkipLaunchSmoke $SkipLaunchSmoke `
    -SkipMessage "skipping MSI-installed executable launch smoke"

Write-Host "==> smoke-uninstalling MSI package"
Stop-TerminalTilerSmokeProcesses
Start-Sleep -Seconds 2
Invoke-MsiExecWithRetry -ArgumentList @("/x", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$MsiInstallRoot") -Label "MSI uninstall"

if (Test-Path $MsiInstalledExe) {
    throw "MSI uninstall left $MsiInstalledExe behind"
}

Write-Host "Windows smoke test passed"
