param(
    [switch]$RequireInstallers = $true
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-installer-tools.ps1")

Assert-WindowsInstallerTools -RequireInstallers:$RequireInstallers | Out-Null
