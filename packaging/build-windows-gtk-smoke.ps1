param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [string]$GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT,
    [switch]$InstallGtkWithGvsbuild,
    [switch]$SkipLaunchSmoke,
    [switch]$RequireInstallers
)

$ErrorActionPreference = "Stop"
$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

$setupArgs = @{}
if ($GtkRuntimeRoot) { $setupArgs.GtkRuntimeRoot = $GtkRuntimeRoot }
if ($InstallGtkWithGvsbuild) { $setupArgs.InstallWithGvsbuild = $true; $setupArgs.SkipBuildIfPresent = $true }
& (Join-Path $PSScriptRoot "setup-windows-gtk.ps1") @setupArgs

$buildArgs = @{
    UseGtkShell = $true
    GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT
}
if ($PackageVersion) { $buildArgs.PackageVersion = $PackageVersion }
if ($RequireInstallers) { $buildArgs.RequireInstallers = $true }
& (Join-Path $PSScriptRoot "build-windows.ps1") @buildArgs

$smokeArgs = @{
    UseGtkShell = $true
    GtkRuntimeRoot = $env:TERMINALTILER_GTK_RUNTIME_ROOT
    SmokeProfileKind = "terminal-only"
    SkipBuild = $true
}
if ($PackageVersion) { $smokeArgs.PackageVersion = $PackageVersion }
if ($SkipLaunchSmoke) { $smokeArgs.SkipLaunchSmoke = $true }
& (Join-Path $PSScriptRoot "windows-smoke-test.ps1") @smokeArgs
