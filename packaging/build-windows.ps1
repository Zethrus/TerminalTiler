param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [switch]$UseGtkShell,
    [switch]$UseWin32Shell,
    [string]$GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT,
    [switch]$SkipCargoBuild,
    [switch]$RequireInstallers
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-installer-tools.ps1")

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

function Copy-WindowsGtkResources {
    param(
        [string]$RootDir,
        [string]$PortableRoot
    )

    $ShareRoot = Join-Path $PortableRoot "share"
    $HoverIconRoot = Join-Path $ShareRoot "hover-icons"
    New-Item -ItemType Directory -Force -Path $HoverIconRoot | Out-Null

    Copy-Item -Path (Join-Path $RootDir "resources\style.css") -Destination (Join-Path $ShareRoot "style.css") -Force
    Copy-Item -Path (Join-Path $RootDir "resources\terminaltiler.svg") -Destination (Join-Path $ShareRoot "terminaltiler.svg") -Force
    Copy-Item -Path (Join-Path $RootDir "resources\hover-icons\*.svg") -Destination $HoverIconRoot -Force
}

function Copy-WindowsGtkRuntime {
    param(
        [string]$RuntimeRoot,
        [string]$PortableRoot
    )

    if ([string]::IsNullOrWhiteSpace($RuntimeRoot)) {
        throw "GTK runtime root is required for the canonical Windows GTK payload. Run setup-windows-gtk.ps1 or set TERMINALTILER_GTK_RUNTIME_ROOT. Use -UseWin32Shell only for an explicit fallback build."
    }
    if (-not (Test-Path $RuntimeRoot)) {
        throw "GTK runtime root was not found at $RuntimeRoot"
    }

    Write-Host "==> bundling GTK/libadwaita runtime from $RuntimeRoot"
    $RuntimeBin = Join-Path $RuntimeRoot "bin"
    if (Test-Path $RuntimeBin) {
        Copy-Item -Path (Join-Path $RuntimeBin "*.dll") -Destination $PortableRoot -Force
    }

    foreach ($relative in @(
        "etc",
        "lib\gdk-pixbuf-2.0",
        "lib\gio",
        "lib\gtk-4.0",
        "share\glib-2.0",
        "share\icons",
        "share\themes"
    )) {
        $source = Join-Path $RuntimeRoot $relative
        $destination = Join-Path $PortableRoot $relative
        New-Item -ItemType Directory -Force -Path $destination | Out-Null
        if (Test-Path $source) {
            Copy-Item -Path (Join-Path $source "*") -Destination $destination -Recurse -Force -ErrorAction SilentlyContinue
        }
        else {
            Write-Host "==> GTK runtime path $relative was not present in $RuntimeRoot; staging placeholder directory"
            Set-Content -Path (Join-Path $destination ".terminaltiler-keep") -Value "GTK runtime did not provide this optional directory." -Encoding ASCII
        }
        if (-not (Get-ChildItem -Path $destination -Force -ErrorAction SilentlyContinue | Select-Object -First 1)) {
            Set-Content -Path (Join-Path $destination ".terminaltiler-keep") -Value "GTK runtime provided an empty optional directory." -Encoding ASCII
        }
    }
}

function Assert-Path {
    param([string]$Path, [string]$Description)

    if ([string]::IsNullOrWhiteSpace($Path) -or -not (Test-Path $Path)) {
        throw "$Description was not found at $Path"
    }
}

function Assert-WindowsStagedPayload {
    param(
        [string]$PortableRoot,
        [switch]$RequireGtkRuntime
    )

    Assert-Path -Path (Join-Path $PortableRoot "TerminalTiler.exe") -Description "Staged TerminalTiler executable"
    Assert-Path -Path (Join-Path $PortableRoot "share\style.css") -Description "Staged canonical GTK CSS"
    Assert-Path -Path (Join-Path $PortableRoot "share\terminaltiler.svg") -Description "Staged TerminalTiler logo"
    Assert-Path -Path (Join-Path $PortableRoot "share\hover-icons\terminal.svg") -Description "Staged terminal hover icon"
    Assert-Path -Path (Join-Path $PortableRoot "share\hover-icons\layout-dashboard.svg") -Description "Staged dashboard hover icon"
    Assert-Path -Path (Join-Path $PortableRoot "share\hover-icons\save.svg") -Description "Staged save hover icon"

    if ($RequireGtkRuntime) {
        if (-not (Get-ChildItem -Path $PortableRoot -Filter "*gtk-4*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1)) {
            throw "Staged GTK4 runtime DLL was not found in $PortableRoot"
        }
        if (-not (Get-ChildItem -Path $PortableRoot -Filter "*adwaita*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1)) {
            throw "Staged libadwaita runtime DLL was not found in $PortableRoot"
        }
        foreach ($relative in @(
            "etc",
            "lib\gdk-pixbuf-2.0",
            "lib\gio",
            "lib\gtk-4.0",
            "share\glib-2.0",
            "share\icons",
            "share\themes"
        )) {
            Assert-Path -Path (Join-Path $PortableRoot $relative) -Description "Staged GTK runtime resource $relative"
        }
    }
}

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$TargetTriple = "x86_64-pc-windows-msvc"
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $RootDir "target" }
$BinaryPath = Join-Path $TargetDir "$TargetTriple\release\terminaltiler.exe"
$StageRoot = Join-Path $RootDir "packaging\.build\windows-stage"
$PortableRoot = Join-Path $StageRoot "portable"
$DistDir = Join-Path $RootDir "dist"
$PortableExePath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-portable-x86_64.exe"
$PortableExeLatestPath = Join-Path $DistDir "TerminalTiler-latest-portable-x86_64.exe"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$ZipLatestPath = Join-Path $DistDir "TerminalTiler-latest-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$InstallerLatestPath = Join-Path $DistDir "TerminalTiler-setup-latest-x86_64.exe"
$MsiPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.msi"
$MsiLatestPath = Join-Path $DistDir "TerminalTiler-setup-latest-x86_64.msi"
$NsisScript = Join-Path $RootDir "packaging\windows\installer.nsi"
$PortableNsisScript = Join-Path $RootDir "packaging\windows\portable.nsi"
$WixScript = Join-Path $RootDir "packaging\windows\installer.wxs"

if ($UseGtkShell -and $UseWin32Shell) {
    throw "Use either the canonical GTK shell or the explicit Win32 fallback, not both."
}

$BuildGtkShell = -not $UseWin32Shell
if ($UseWin32Shell) {
    Write-Host "==> explicit Win32 fallback build requested"
}
else {
    Write-Host "==> canonical Windows GTK/libadwaita shell build selected"
}

if (-not $SkipCargoBuild) {
    Write-Host "==> building Windows release binary"
    $BuildFeatures = @("voice-cpal")
    if ($BuildGtkShell) {
        $BuildFeatures += "windows-gtk-shell"
    }
    cargo build --release --features ($BuildFeatures -join ",") --target $TargetTriple --manifest-path (Join-Path $RootDir "Cargo.toml")
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
Copy-WindowsGtkResources -RootDir $RootDir -PortableRoot $PortableRoot
if ($BuildGtkShell) {
    Copy-WindowsGtkRuntime -RuntimeRoot $GtkRuntimeRoot -PortableRoot $PortableRoot
}

$ReadmePath = Join-Path $PortableRoot "README-windows.txt"
@"
TerminalTiler for Windows
=========================

Shell:
- GTK/libadwaita is the canonical parity shell when the package is built with
  the default packaging path. It loads the same style.css, TerminalTiler logo,
  and hover icon payload as the Ubuntu GTK build.
- The Win32 shell remains available as an internal compatibility fallback when
  the package is built with -UseWin32Shell or the windows-win32-shell cargo
  feature.

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

Assert-WindowsStagedPayload -PortableRoot $PortableRoot -RequireGtkRuntime:$BuildGtkShell

$InstallerTools = Assert-WindowsInstallerTools -RequireInstallers:$RequireInstallers
$Makensis = $InstallerTools.Makensis
$Candle = $InstallerTools.Candle
$Light = $InstallerTools.Light
$Heat = $InstallerTools.Heat

if (-not $Makensis) {
    throw "NSIS is required to build the direct portable self-extracting executable"
}

Write-Host "==> publishing direct portable executable"
Remove-Item -Force $PortableExePath, $PortableExeLatestPath -ErrorAction SilentlyContinue
& $Makensis `
    "/DAPP_VERSION=$ResolvedVersion" `
    "/DSTAGE_DIR=$PortableRoot" `
    "/DOUT_FILE=$PortableExePath" `
    $PortableNsisScript

if ($LASTEXITCODE -ne 0) {
    throw "NSIS failed while building portable executable $PortableExePath"
}
if (-not (Test-Path $PortableExePath)) {
    throw "Expected portable executable was not created at $PortableExePath"
}

Copy-Item -Path $PortableExePath -Destination $PortableExeLatestPath -Force

Write-Host "==> creating portable zip"
Remove-Item -Force $ZipPath, $ZipLatestPath -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $PortableRoot "*") -DestinationPath $ZipPath -Force
Copy-Item -Path $ZipPath -Destination $ZipLatestPath -Force

if ($Makensis) {
    Write-Host "==> building NSIS installer"
    Remove-Item -Force $InstallerPath, $InstallerLatestPath -ErrorAction SilentlyContinue
    & $Makensis `
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

if ($Candle -and $Light -and $Heat) {
    Write-Host "==> building MSI installer"
    $WixBuildDir = Join-Path $StageRoot "wix"
    $WixHarvestedSourcePath = Join-Path $WixBuildDir "harvested-payload.wxs"
    $WixObjectPath = Join-Path $WixBuildDir "terminaltiler-installer.wixobj"
    $WixHarvestedObjectPath = Join-Path $WixBuildDir "harvested-payload.wixobj"

    New-Item -ItemType Directory -Force -Path $WixBuildDir | Out-Null
    Remove-Item -Force $WixHarvestedSourcePath, $WixObjectPath, $WixHarvestedObjectPath, $MsiPath, $MsiLatestPath -ErrorAction SilentlyContinue

    & $Heat `
        "dir" $PortableRoot `
        "-nologo" `
        "-cg" "HarvestedPayloadComponents" `
        "-dr" "INSTALLFOLDER" `
        "-srd" `
        "-sreg" `
        "-scom" `
        "-gg" `
        "-var" "var.StageDir" `
        "-out" $WixHarvestedSourcePath

    if ($LASTEXITCODE -ne 0) {
        throw "WiX heat failed while harvesting $PortableRoot"
    }

    & $Candle `
        "-nologo" `
        "-arch" "x64" `
        "-dProductVersion=$ResolvedVersion" `
        "-dStageDir=$PortableRoot" `
        "-out" $WixObjectPath `
        $WixScript

    if ($LASTEXITCODE -ne 0) {
        throw "WiX candle failed while compiling $WixScript"
    }

    & $Candle `
        "-nologo" `
        "-arch" "x64" `
        "-dStageDir=$PortableRoot" `
        "-out" $WixHarvestedObjectPath `
        $WixHarvestedSourcePath

    if ($LASTEXITCODE -ne 0) {
        throw "WiX candle failed while compiling $WixHarvestedSourcePath"
    }

    # Harvested payload files install under the per-user LocalAppDataFolder
    # tree. They intentionally use file key paths so future staged files are
    # packaged without hand-authored HKCU registry keys for every component.
    & $Light `
        "-nologo" `
        "-sice:ICE38" `
        "-sice:ICE64" `
        "-sice:ICE91" `
        "-out" $MsiPath `
        $WixObjectPath `
        $WixHarvestedObjectPath

    if ($LASTEXITCODE -ne 0) {
        throw "WiX light failed while linking $MsiPath"
    }

    if (-not (Test-Path $MsiPath)) {
        throw "Expected MSI installer was not created at $MsiPath"
    }

    Copy-Item -Path $MsiPath -Destination $MsiLatestPath -Force
} else {
    Write-Host "==> WiX Toolset not found - skipping MSI build"
    Write-Host "    To build the MSI, install WiX Toolset from https://wixtoolset.org/"
}

Save-SuccessfulBuildVersion -Version $ResolvedVersion
Write-Host "Windows packaging complete"
Write-Host "  portable exe: $PortableExePath"
Write-Host "  zip: $ZipPath"
if ($Makensis) {
    Write-Host "  installer: $InstallerPath"
}
if ($Candle -and $Light -and $Heat) {
    Write-Host "  msi: $MsiPath"
}
