param(
    [string]$PackageVersion = $env:PACKAGE_VERSION,
    [string]$DistDir,
    [string]$OutputDir = (Join-Path $PSScriptRoot ".build\windows-gtk-release-visuals"),
    [string]$StageRoot = (Join-Path $PSScriptRoot ".build\windows-gtk-release-visual-stage"),
    [ValidateSet("launch-dashboard", "saved-workspaces", "restored-workspace", "workspace-with-web")]
    [string[]]$CaptureSet = @("launch-dashboard", "saved-workspaces", "restored-workspace", "workspace-with-web"),
    [ValidateSet("system", "light", "dark")]
    [string]$Theme = "dark",
    [ValidateSet("comfortable", "standard", "compact")]
    [string]$Density = "compact",
    [int]$StartupTimeoutSeconds = 20,
    [switch]$KeepInstalled
)

$ErrorActionPreference = "Stop"

$RootDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($DistDir)) {
    $DistDir = Join-Path $RootDir "dist"
}

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

function Invoke-ArtifactCapture {
    param(
        [string]$Label,
        [string]$ExePath
    )

    Assert-Path -Path $ExePath -Description "$Label executable"
    Write-Host "==> capturing Windows GTK visuals for $Label from $ExePath"
    & (Join-Path $PSScriptRoot "capture-windows-gtk-visuals.ps1") `
        -ExePath $ExePath `
        -OutputDir (Join-Path $OutputDir $Label) `
        -CaptureSet $CaptureSet `
        -Theme $Theme `
        -Density $Density `
        -StartupTimeoutSeconds $StartupTimeoutSeconds
}

function Install-NsisArtifact {
    param(
        [string]$InstallerPath,
        [string]$InstallRoot
    )

    Remove-Item -Recurse -Force $InstallRoot -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    $installerProcess = Start-Process -FilePath $InstallerPath -ArgumentList @("/S", "/D=$InstallRoot") -PassThru -Wait
    if ($installerProcess.ExitCode -ne 0) {
        throw "NSIS installer exited with code $($installerProcess.ExitCode)"
    }

    return Join-Path $InstallRoot "TerminalTiler.exe"
}

function Install-MsiArtifact {
    param(
        [string]$MsiPath,
        [string]$InstallRoot
    )

    Remove-Item -Recurse -Force $InstallRoot -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    $msiProcess = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/i", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$InstallRoot") -PassThru -Wait
    if ($msiProcess.ExitCode -ne 0) {
        throw "MSI installer exited with code $($msiProcess.ExitCode)"
    }

    return Join-Path $InstallRoot "TerminalTiler.exe"
}

function Uninstall-NsisArtifact {
    param([string]$InstallRoot)

    $uninstaller = Join-Path $InstallRoot "Uninstall.exe"
    if (Test-Path $uninstaller) {
        $process = Start-Process -FilePath $uninstaller -ArgumentList @("/S") -PassThru -Wait
        if ($process.ExitCode -ne 0) {
            Write-Warning "NSIS uninstaller exited with code $($process.ExitCode)"
        }
    }
    Remove-Item -Recurse -Force $InstallRoot -ErrorAction SilentlyContinue
}

function Uninstall-MsiArtifact {
    param(
        [string]$MsiPath,
        [string]$InstallRoot
    )

    $process = Start-Process -FilePath "msiexec.exe" -ArgumentList @("/x", $MsiPath, "/qn", "/norestart", "INSTALLFOLDER=$InstallRoot") -PassThru -Wait
    if ($process.ExitCode -ne 0) {
        Write-Warning "MSI uninstall exited with code $($process.ExitCode)"
    }
    Remove-Item -Recurse -Force $InstallRoot -ErrorAction SilentlyContinue
}

$ResolvedVersion = Get-PackageVersion -RootDir $RootDir
$PortableExePath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-portable-x86_64.exe"
$ZipPath = Join-Path $DistDir "TerminalTiler-$ResolvedVersion-windows-x86_64.zip"
$InstallerPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.exe"
$MsiPath = Join-Path $DistDir "TerminalTiler-setup-$ResolvedVersion-x86_64.msi"

Assert-Path -Path $PortableExePath -Description "Portable self-extracting executable"
Assert-Path -Path $ZipPath -Description "Portable zip"
Assert-Path -Path $InstallerPath -Description "NSIS installer"
Assert-Path -Path $MsiPath -Description "MSI installer"

Remove-Item -Recurse -Force $StageRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $OutputDir, $StageRoot | Out-Null

Invoke-ArtifactCapture -Label "portable-exe" -ExePath $PortableExePath

$zipRoot = Join-Path $StageRoot "portable-zip"
New-Item -ItemType Directory -Force -Path $zipRoot | Out-Null
Expand-Archive -Path $ZipPath -DestinationPath $zipRoot -Force
Invoke-ArtifactCapture -Label "portable-zip" -ExePath (Join-Path $zipRoot "TerminalTiler.exe")

$nsisInstallRoot = Join-Path $StageRoot "nsis-install"
$nsisExe = Install-NsisArtifact -InstallerPath $InstallerPath -InstallRoot $nsisInstallRoot
try {
    Invoke-ArtifactCapture -Label "nsis-install" -ExePath $nsisExe
}
finally {
    if (-not $KeepInstalled) {
        Uninstall-NsisArtifact -InstallRoot $nsisInstallRoot
    }
}

$msiInstallRoot = Join-Path $StageRoot "msi-install"
$msiExe = Install-MsiArtifact -MsiPath $MsiPath -InstallRoot $msiInstallRoot
try {
    Invoke-ArtifactCapture -Label "msi-install" -ExePath $msiExe
}
finally {
    if (-not $KeepInstalled) {
        Uninstall-MsiArtifact -MsiPath $MsiPath -InstallRoot $msiInstallRoot
    }
}

Write-Host "Windows release GTK visual captures written to $OutputDir"
