param(
    [string]$ExePath,
    [string]$OutputDir = (Join-Path $PSScriptRoot ".build\windows-gtk-visuals"),
    [ValidateSet("launch-dashboard", "saved-workspaces", "restored-workspace", "workspace-with-web")]
    [string[]]$CaptureSet = @("launch-dashboard", "saved-workspaces", "restored-workspace", "workspace-with-web"),
    [ValidateSet("system", "light", "dark")]
    [string]$Theme = "dark",
    [ValidateSet("comfortable", "standard", "compact")]
    [string]$Density = "compact",
    [int]$StartupTimeoutSeconds = 20,
    [switch]$KeepProcess
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ExePath)) {
    $RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
    $lastSuccessfulVersionFile = Join-Path $RootDir "packaging\.build\versioning\last-successful-version"
    $version = if (Test-Path $lastSuccessfulVersionFile) {
        (Get-Content -Path $lastSuccessfulVersionFile -Raw).Trim()
    } else {
        $cargoToml = Get-Content -Path (Join-Path $RootDir "Cargo.toml") -Raw
        [regex]::Match($cargoToml, '(?ms)^\[package\].*?^version = "([^"]+)"').Groups[1].Value
    }
    $ExePath = Join-Path $RootDir "dist\TerminalTiler-$version-portable-x86_64.exe"
}

if (-not (Test-Path $ExePath)) {
    throw "TerminalTiler executable was not found at $ExePath"
}

Add-Type -AssemblyName System.Drawing
if (-not ("WindowCaptureNative" -as [type])) {
Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class WindowCaptureNative {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc enumProc, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint flags);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }
}
"@
}

function Convert-ToTomlPath {
    param([string]$Path)
    return ($Path -replace '\\', '\\\\')
}

function Initialize-VisualProfile {
    param(
        [string]$SandboxRoot,
        [string]$Scenario,
        [string]$Theme,
        [string]$Density
    )

    $workspaceRoot = Join-Path $SandboxRoot "workspace"
    $profileRoot = Join-Path $SandboxRoot "profile"
    $configRoot = Join-Path $profileRoot "config"
    $dataRoot = Join-Path $profileRoot "data"
    $logsRoot = Join-Path $profileRoot "state\logs"

    foreach ($dir in @($workspaceRoot, $configRoot, $dataRoot, $logsRoot)) {
        New-Item -ItemType Directory -Force -Path $dir | Out-Null
    }

    $restoreMode = if ($Scenario -in @("restored-workspace", "workspace-with-web")) { "shell-only" } else { "prompt" }
    @"
version = 1
default_restore_mode = "$restoreMode"
default_theme = "$Theme"
default_density = "$Density"
"@ | Set-Content -Path (Join-Path $configRoot "preferences.toml") -Encoding ASCII

    $workspacePath = Convert-ToTomlPath -Path $workspaceRoot
    if ($Scenario -eq "saved-workspaces") {
        @"
version = 1

[[presets]]
id = "visual-qa-saved-fleet"
name = "Visual QA Saved Fleet"
description = "Seeded saved workspace card for Linux and Windows visual parity review."
tags = ["visual", "qa", "saved"]
root_label = "QA workspace"
workspace_root = "$workspacePath"
theme = "$Theme"
density = "$Density"

[presets.layout]
kind = "split"
axis = "horizontal"
ratio = 0.55

[presets.layout.first]
kind = "tile"
id = "saved-builder"
title = "Builder"
agent_label = "Build"
accent_class = "accent-cyan"

[presets.layout.first.working_directory]
type = "workspace-root"

[presets.layout.second]
kind = "tile"
id = "saved-reviewer"
title = "Reviewer"
agent_label = "QA"
accent_class = "accent-rose"

[presets.layout.second.working_directory]
type = "workspace-root"

[[presets]]
id = "visual-qa-docs-shell"
name = "Visual QA Docs + Shell"
description = "Seeded web plus terminal card to expose saved tile badges and actions."
tags = ["visual", "web", "shell"]
root_label = "Docs workspace"
workspace_root = "$workspacePath"
theme = "$Theme"
density = "$Density"

[presets.layout]
kind = "split"
axis = "vertical"
ratio = 0.48

[presets.layout.first]
kind = "tile"
id = "saved-docs"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-violet"
tile_kind = "web-view"
url = "about:blank"

[presets.layout.first.working_directory]
type = "workspace-root"

[presets.layout.second]
kind = "tile"
id = "saved-shell"
title = "Shell"
agent_label = "Terminal"
accent_class = "accent-amber"

[presets.layout.second.working_directory]
type = "workspace-root"
"@ | Set-Content -Path (Join-Path $configRoot "presets.toml") -Encoding ASCII
    }

    if ($Scenario -eq "restored-workspace") {
        @"
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspacePath"
custom_title = "Visual QA Restore"
terminal_zoom_steps = 0

[tabs.preset]
id = "visual-qa-restore"
name = "Visual QA Restore"
description = "Visual QA restored workspace"
tags = ["visual", "qa"]
root_label = "Workspace root"
theme = "$Theme"
density = "$Density"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.5

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-primary"
title = "Primary"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "split"
axis = "vertical"
ratio = 0.5

[tabs.preset.layout.second.first]
kind = "tile"
id = "terminal-secondary"
title = "Secondary"
agent_label = "Agent"
accent_class = "accent-purple"

[tabs.preset.layout.second.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second.second]
kind = "tile"
id = "terminal-logs"
title = "Logs"
agent_label = "Monitor"
accent_class = "accent-amber"

[tabs.preset.layout.second.second.working_directory]
type = "workspace-root"
"@ | Set-Content -Path (Join-Path $dataRoot "session.toml") -Encoding ASCII
    }

    if ($Scenario -eq "workspace-with-web") {
        @"
version = 1
active_tab_index = 0

[[tabs]]
workspace_root = "$workspacePath"
custom_title = "Visual QA Web Workspace"
terminal_zoom_steps = 0

[tabs.preset]
id = "visual-qa-web-workspace"
name = "Visual QA Web Workspace"
description = "Visual QA restored web and terminal workspace"
tags = ["visual", "qa", "web"]
root_label = "Workspace root"
theme = "$Theme"
density = "$Density"

[tabs.preset.layout]
kind = "split"
axis = "horizontal"
ratio = 0.52

[tabs.preset.layout.first]
kind = "tile"
id = "terminal-control"
title = "Control"
agent_label = "Shell"
accent_class = "accent-cyan"

[tabs.preset.layout.first.working_directory]
type = "workspace-root"

[tabs.preset.layout.second]
kind = "tile"
id = "web-docs"
title = "Docs"
agent_label = "Browser"
accent_class = "accent-violet"
tile_kind = "web-view"
url = "about:blank"

[tabs.preset.layout.second.working_directory]
type = "workspace-root"
"@ | Set-Content -Path (Join-Path $dataRoot "session.toml") -Encoding ASCII
    }

    return @{
        ProfileRoot = $profileRoot
        AppData = Join-Path $SandboxRoot "AppData\Roaming"
        LocalAppData = Join-Path $SandboxRoot "AppData\Local"
        UserProfile = Join-Path $SandboxRoot "User"
    }
}

function Get-ProcessWindows {
    param([int[]]$ProcessIds)

    $windows = New-Object System.Collections.Generic.List[object]
    $callback = [WindowCaptureNative+EnumWindowsProc]{
        param([IntPtr]$hWnd, [IntPtr]$lParam)
        if (-not [WindowCaptureNative]::IsWindowVisible($hWnd)) { return $true }
        $pid = [uint32]0
        [void][WindowCaptureNative]::GetWindowThreadProcessId($hWnd, [ref]$pid)
        if ($ProcessIds -notcontains [int]$pid) { return $true }
        $titleBuilder = New-Object System.Text.StringBuilder 512
        [void][WindowCaptureNative]::GetWindowText($hWnd, $titleBuilder, $titleBuilder.Capacity)
        $title = $titleBuilder.ToString()
        if ([string]::IsNullOrWhiteSpace($title)) { $title = "window" }
        $rect = New-Object WindowCaptureNative+RECT
        if ([WindowCaptureNative]::GetWindowRect($hWnd, [ref]$rect) -and ($rect.Right -gt $rect.Left) -and ($rect.Bottom -gt $rect.Top)) {
            $windows.Add([pscustomobject]@{ Handle = $hWnd; Title = $title; Rect = $rect }) | Out-Null
        }
        return $true
    }
    [void][WindowCaptureNative]::EnumWindows($callback, [IntPtr]::Zero)
    return @($windows)
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

function Get-ProcessTreeIds {
    param([int]$RootProcessId)

    return @($RootProcessId) + @(Get-DescendantProcessIds -RootProcessId $RootProcessId)
}

function Stop-ProcessTree {
    param([System.Diagnostics.Process]$Process)

    $processIds = @(Get-DescendantProcessIds -RootProcessId $Process.Id)
    [array]::Reverse($processIds)
    foreach ($processId in $processIds) {
        Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
    }

    if (-not $Process.HasExited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
    }
}

function Save-WindowPng {
    param([object]$Window, [string]$Path)

    $width = $Window.Rect.Right - $Window.Rect.Left
    $height = $Window.Rect.Bottom - $Window.Rect.Top
    $bitmap = New-Object System.Drawing.Bitmap $width, $height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $hdc = $graphics.GetHdc()
        try {
            $printed = [WindowCaptureNative]::PrintWindow($Window.Handle, $hdc, 2)
        }
        finally {
            $graphics.ReleaseHdc($hdc)
        }
        if (-not $printed) {
            $graphics.CopyFromScreen($Window.Rect.Left, $Window.Rect.Top, 0, 0, $bitmap.Size)
        }
        $bitmap.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Wait-ForWindows {
    param([System.Diagnostics.Process]$Process, [int]$TimeoutSeconds)

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        $Process.Refresh()
        $processIds = @(Get-ProcessTreeIds -RootProcessId $Process.Id)
        $descendantIds = @($processIds | Where-Object { $_ -ne $Process.Id })
        if ($Process.HasExited -and $descendantIds.Count -eq 0) {
            throw "TerminalTiler exited before visual capture with code $($Process.ExitCode)"
        }
        $windows = @(Get-ProcessWindows -ProcessIds $processIds)
        if ($windows.Count -gt 0) { return $windows }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)

    throw "Timed out waiting for TerminalTiler windows"
}

function Invoke-VisualCaptureScenario {
    param([string]$Scenario)

    $scenarioRoot = Join-Path $OutputDir $Scenario
    $sandboxRoot = Join-Path $scenarioRoot "sandbox"
    $captureRoot = Join-Path $scenarioRoot "captures"
    Remove-Item -Recurse -Force $sandboxRoot, $captureRoot -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $captureRoot | Out-Null
    $profile = Initialize-VisualProfile -SandboxRoot $sandboxRoot -Scenario $Scenario -Theme $Theme -Density $Density

    $previousEnvironment = @{
        APPDATA = $env:APPDATA
        LOCALAPPDATA = $env:LOCALAPPDATA
        USERPROFILE = $env:USERPROFILE
        HOME = $env:HOME
        TERMINALTILER_PROFILE_ROOT = $env:TERMINALTILER_PROFILE_ROOT
    }
    $process = $null
    try {
        foreach ($dir in @($profile.AppData, $profile.LocalAppData, $profile.UserProfile)) {
            New-Item -ItemType Directory -Force -Path $dir | Out-Null
        }
        $env:APPDATA = $profile.AppData
        $env:LOCALAPPDATA = $profile.LocalAppData
        $env:USERPROFILE = $profile.UserProfile
        $env:HOME = $profile.UserProfile
        $env:TERMINALTILER_PROFILE_ROOT = $profile.ProfileRoot

        $process = Start-Process -FilePath $ExePath -PassThru
        Start-Sleep -Seconds 3
        $windows = Wait-ForWindows -Process $process -TimeoutSeconds $StartupTimeoutSeconds
        $index = 0
        foreach ($window in $windows) {
            $safeTitle = ($window.Title -replace '[^A-Za-z0-9._-]+', '-').Trim('-')
            if ([string]::IsNullOrWhiteSpace($safeTitle)) { $safeTitle = "window" }
            $path = Join-Path $captureRoot ("{0:D2}-{1}-{2}-{3}-{4}.png" -f $index, $Scenario, $Theme, $Density, $safeTitle)
            Save-WindowPng -Window $window -Path $path
            Write-Host "Captured $path"
            $index++
        }
    }
    finally {
        if ($process -and -not $KeepProcess) {
            Stop-ProcessTree -Process $process
        }
        $env:APPDATA = $previousEnvironment.APPDATA
        $env:LOCALAPPDATA = $previousEnvironment.LOCALAPPDATA
        $env:USERPROFILE = $previousEnvironment.USERPROFILE
        $env:HOME = $previousEnvironment.HOME
        $env:TERMINALTILER_PROFILE_ROOT = $previousEnvironment.TERMINALTILER_PROFILE_ROOT
    }
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
foreach ($scenario in $CaptureSet) {
    Invoke-VisualCaptureScenario -Scenario $scenario
}

Write-Host "Windows GTK visual captures written to $OutputDir"
