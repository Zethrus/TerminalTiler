param(
    [string]$GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT,
    [switch]$InstallWithGvsbuild,
    [string]$GvsbuildBuildRoot = "C:\gtk-build",
    [switch]$SkipBuildIfPresent
)

$ErrorActionPreference = "Stop"

function Add-PathPrefix {
    param([string]$PathPrefix)
    if ([string]::IsNullOrWhiteSpace($PathPrefix) -or -not (Test-Path $PathPrefix)) {
        return
    }
    $current = [Environment]::GetEnvironmentVariable("Path", "Process")
    if (-not ($current.Split(';') -contains $PathPrefix)) {
        [Environment]::SetEnvironmentVariable("Path", "$PathPrefix;$current", "Process")
    }
}

function Add-EnvPrefix {
    param([string]$Name, [string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value) -or -not (Test-Path $Value)) {
        return
    }
    $current = [Environment]::GetEnvironmentVariable($Name, "Process")
    if ([string]::IsNullOrWhiteSpace($current)) {
        [Environment]::SetEnvironmentVariable($Name, $Value, "Process")
    }
    elseif (-not ($current.Split(';') -contains $Value)) {
        [Environment]::SetEnvironmentVariable($Name, "$Value;$current", "Process")
    }
}


function Invoke-WithRetry {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$ScriptBlock,
        [Parameter(Mandatory = $true)][string]$Description,
        [int]$Attempts = 3,
        [int]$DelaySeconds = 20
    )

    for ($attempt = 1; $attempt -le $Attempts; $attempt++) {
        try {
            Write-Host "==> $Description (attempt $attempt/$Attempts)"
            & $ScriptBlock
            return
        }
        catch {
            if ($attempt -ge $Attempts) {
                throw
            }

            $sleepSeconds = $DelaySeconds * $attempt
            Write-Warning "$Description failed on attempt $($attempt)/$($Attempts): $($_.Exception.Message)"
            Write-Warning "Retrying in $sleepSeconds seconds..."
            Start-Sleep -Seconds $sleepSeconds
        }
    }
}

function Invoke-WithProcessEnvironment {
    param(
        [Parameter(Mandatory = $true)][hashtable]$Variables,
        [Parameter(Mandatory = $true)][scriptblock]$ScriptBlock
    )

    $previousValues = @{}
    foreach ($entry in $Variables.GetEnumerator()) {
        $previousValues[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, "Process")
        [Environment]::SetEnvironmentVariable($entry.Key, [string]$entry.Value, "Process")
    }

    try {
        & $ScriptBlock
    }
    finally {
        foreach ($entry in $previousValues.GetEnumerator()) {
            [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, "Process")
        }
    }
}

function Save-HicolorIconThemeArchive {
    param([string]$BuildRoot)

    $srcRoot = Join-Path $BuildRoot "src"
    $archivePath = Join-Path $srcRoot "hicolor-icon-theme-0.18.tar.xz"
    New-Item -ItemType Directory -Force -Path $srcRoot | Out-Null

    if ((Test-Path $archivePath) -and ((Get-Item $archivePath).Length -gt 0)) {
        Write-Host "==> hicolor icon theme archive already cached at $archivePath"
        return
    }

    $urls = @(
        "https://icon-theme.freedesktop.org/releases/hicolor-icon-theme-0.18.tar.xz",
        "https://www.freedesktop.org/software/icon-theme/releases/hicolor-icon-theme-0.18.tar.xz",
        "https://distfiles.macports.org/hicolor-icon-theme/hicolor-icon-theme-0.18.tar.xz"
    )

    foreach ($url in $urls) {
        try {
            Remove-Item -Force $archivePath -ErrorAction SilentlyContinue
            Invoke-WithRetry `
                -Description "prefetch hicolor-icon-theme from $url" `
                -Attempts 2 `
                -DelaySeconds 15 `
                -ScriptBlock {
                    Invoke-WebRequest -Uri $url -OutFile $archivePath -UseBasicParsing -TimeoutSec 120
                    if (-not (Test-Path $archivePath) -or ((Get-Item $archivePath).Length -le 0)) {
                        throw "Downloaded hicolor icon theme archive is empty"
                    }
                }
            return
        }
        catch {
            Write-Warning "Unable to prefetch hicolor icon theme from ${url}: $($_.Exception.Message)"
        }
    }

    throw "Unable to prefetch hicolor-icon-theme-0.18.tar.xz for gvsbuild"
}

function Resolve-GtkRuntimeRoot {
    param([string]$Candidate)

    foreach ($path in @($Candidate, "C:\gtk", "C:\gtk-build\gtk\x64\release")) {
        if (
            -not [string]::IsNullOrWhiteSpace($path) -and
            (Test-Path (Join-Path $path "bin")) -and
            (Get-ChildItem -Path (Join-Path $path "bin") -Filter "*gtk-4*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1) -and
            (Get-ChildItem -Path (Join-Path $path "bin") -Filter "*adwaita*.dll" -ErrorAction SilentlyContinue | Select-Object -First 1)
        ) {
            return (Resolve-Path $path).Path
        }
    }

    return $null
}

function Install-GtkWithGvsbuild {
    param([string]$BuildRoot)

    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        throw "Python is required to install gvsbuild for Windows GTK setup."
    }

    New-Item -ItemType Directory -Force -Path $BuildRoot | Out-Null
    Save-HicolorIconThemeArchive -BuildRoot $BuildRoot

    Invoke-WithRetry -Description "upgrade pip" -ScriptBlock {
        python -m pip install --upgrade pip
        if ($LASTEXITCODE -ne 0) {
            throw "pip upgrade failed with exit code $LASTEXITCODE"
        }
    }
    Invoke-WithRetry -Description "install gvsbuild" -ScriptBlock {
        python -m pip install --user --upgrade gvsbuild
        if ($LASTEXITCODE -ne 0) {
            throw "gvsbuild pip install failed with exit code $LASTEXITCODE"
        }
    }

    $scriptRoots = Get-ChildItem -Path (Join-Path $env:APPDATA "Python") -Filter "Scripts" -Directory -Recurse -ErrorAction SilentlyContinue
    foreach ($scriptRoot in $scriptRoots) {
        Add-PathPrefix -PathPrefix $scriptRoot.FullName
    }
    $gvsbuild = Get-Command gvsbuild -ErrorAction Stop
    $gvsbuildCargoHome = Join-Path $BuildRoot "tools\cargo"
    New-Item -ItemType Directory -Force -Path $gvsbuildCargoHome | Out-Null

    # gvsbuild builds Rust-based GTK dependencies such as librsvg with its own
    # rustup/cargo toolchain. GitHub Actions checks out TerminalTiler before this
    # script runs, so the repository rust-toolchain.toml can otherwise force
    # gvsbuild's transient `cargo install cargo-c --locked` onto the project MSRV
    # toolchain. cargo-c tracks recent Cargo internals and currently requires a
    # newer compiler than TerminalTiler itself, so isolate only the gvsbuild
    # subprocesses onto stable without exporting that override to later CI steps.
    # The hosted runner also exports CARGO_HOME for the project toolchain; point
    # gvsbuild at its own cargo home so cargo-cbuild lands beside gvsbuild's
    # cargo.exe and Meson can find it when configuring librsvg.
    Invoke-WithProcessEnvironment -Variables @{
        RUSTUP_TOOLCHAIN = "stable"
        RUSTUP_HOME      = $gvsbuildCargoHome
        CARGO_HOME       = $gvsbuildCargoHome
    } -ScriptBlock {
        Invoke-WithRetry -Description "build GTK4/libadwaita with gvsbuild" -Attempts 2 -DelaySeconds 60 -ScriptBlock {
            & $gvsbuild.Source build --build-dir $BuildRoot --configuration release gtk4 libadwaita librsvg adwaita-icon-theme
            if ($LASTEXITCODE -ne 0) {
                throw "gvsbuild failed with exit code $LASTEXITCODE"
            }
        }
    }
}

$ResolvedRoot = Resolve-GtkRuntimeRoot -Candidate $GtkRuntimeRoot
if (-not $ResolvedRoot -and $InstallWithGvsbuild) {
    if ($SkipBuildIfPresent -and (Resolve-GtkRuntimeRoot -Candidate (Join-Path $GvsbuildBuildRoot "gtk\x64\release"))) {
        Write-Host "==> using cached gvsbuild GTK runtime"
    }
    else {
        Write-Host "==> building GTK4/libadwaita runtime with gvsbuild"
        Install-GtkWithGvsbuild -BuildRoot $GvsbuildBuildRoot
    }
    $ResolvedRoot = Resolve-GtkRuntimeRoot -Candidate (Join-Path $GvsbuildBuildRoot "gtk\x64\release")
}

if (-not $ResolvedRoot) {
    throw "GTK runtime not found. Set TERMINALTILER_GTK_RUNTIME_ROOT, install C:\gtk, or rerun with -InstallWithGvsbuild."
}

$env:TERMINALTILER_GTK_RUNTIME_ROOT = $ResolvedRoot
Add-PathPrefix -PathPrefix (Join-Path $ResolvedRoot "bin")
Add-EnvPrefix -Name "LIB" -Value (Join-Path $ResolvedRoot "lib")
Add-EnvPrefix -Name "INCLUDE" -Value (Join-Path $ResolvedRoot "include")
Add-EnvPrefix -Name "INCLUDE" -Value (Join-Path $ResolvedRoot "include\cairo")
Add-EnvPrefix -Name "INCLUDE" -Value (Join-Path $ResolvedRoot "include\glib-2.0")
Add-EnvPrefix -Name "INCLUDE" -Value (Join-Path $ResolvedRoot "include\gobject-introspection-1.0")
Add-EnvPrefix -Name "INCLUDE" -Value (Join-Path $ResolvedRoot "lib\glib-2.0\include")
$env:PKG_CONFIG_PATH = Join-Path $ResolvedRoot "lib\pkgconfig"

Write-Host "TERMINALTILER_GTK_RUNTIME_ROOT=$ResolvedRoot"
Write-Host "PKG_CONFIG_PATH=$env:PKG_CONFIG_PATH"

if ($env:GITHUB_ENV) {
    Add-Content -Path $env:GITHUB_ENV -Value "TERMINALTILER_GTK_RUNTIME_ROOT=$ResolvedRoot"
    Add-Content -Path $env:GITHUB_ENV -Value "PKG_CONFIG_PATH=$env:PKG_CONFIG_PATH"
    Add-Content -Path $env:GITHUB_ENV -Value "LIB=$env:LIB"
    Add-Content -Path $env:GITHUB_ENV -Value "INCLUDE=$env:INCLUDE"
}
if ($env:GITHUB_PATH) {
    Add-Content -Path $env:GITHUB_PATH -Value (Join-Path $ResolvedRoot "bin")
}

$pkgConfig = Get-Command pkg-config -ErrorAction Stop
& $pkgConfig.Source --cflags gtk4 --msvc-syntax | Write-Host
& $pkgConfig.Source --modversion gtk4 | Write-Host
& $pkgConfig.Source --modversion libadwaita-1 | Write-Host
