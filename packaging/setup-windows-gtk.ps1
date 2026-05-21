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

    python -m pip install --upgrade pip
    python -m pip install --user --upgrade gvsbuild

    $scriptRoots = Get-ChildItem -Path (Join-Path $env:APPDATA "Python") -Filter "Scripts" -Directory -Recurse -ErrorAction SilentlyContinue
    foreach ($scriptRoot in $scriptRoots) {
        Add-PathPrefix -PathPrefix $scriptRoot.FullName
    }
    $gvsbuild = Get-Command gvsbuild -ErrorAction Stop
    & $gvsbuild.Source build gtk4 libadwaita librsvg adwaita-icon-theme
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
