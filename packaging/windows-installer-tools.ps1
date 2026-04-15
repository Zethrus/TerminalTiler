function Find-ExecutablePath {
    param(
        [string[]]$CommandNames,
        [string[]]$CandidatePaths = @()
    )

    foreach ($commandName in $CommandNames) {
        $command = Get-Command $commandName -ErrorAction SilentlyContinue
        if ($command) {
            return $command.Source
        }
    }

    foreach ($candidate in $CandidatePaths) {
        if ([string]::IsNullOrWhiteSpace($candidate)) {
            continue
        }

        $expanded = [Environment]::ExpandEnvironmentVariables($candidate)
        if (Test-Path $expanded) {
            return (Resolve-Path $expanded).Path
        }
    }

    return $null
}

function Get-WindowsInstallerTools {
    $makensis = Find-ExecutablePath -CommandNames @("makensis.exe", "makensis") -CandidatePaths @(
        "$env:ProgramFiles(x86)\NSIS\makensis.exe",
        "$env:ProgramFiles\NSIS\makensis.exe",
        "$env:ChocolateyInstall\bin\makensis.exe"
    )

    $candle = Find-ExecutablePath -CommandNames @("candle.exe", "candle") -CandidatePaths @(
        "$env:WIX\bin\candle.exe",
        "$env:ProgramFiles(x86)\WiX Toolset v3.14\bin\candle.exe",
        "$env:ProgramFiles\WiX Toolset v3.14\bin\candle.exe",
        "$env:ProgramFiles(x86)\WiX Toolset v3.11\bin\candle.exe",
        "$env:ProgramFiles\WiX Toolset v3.11\bin\candle.exe"
    )

    $light = Find-ExecutablePath -CommandNames @("light.exe", "light") -CandidatePaths @(
        "$env:WIX\bin\light.exe",
        "$env:ProgramFiles(x86)\WiX Toolset v3.14\bin\light.exe",
        "$env:ProgramFiles\WiX Toolset v3.14\bin\light.exe",
        "$env:ProgramFiles(x86)\WiX Toolset v3.11\bin\light.exe",
        "$env:ProgramFiles\WiX Toolset v3.11\bin\light.exe"
    )

    [pscustomobject]@{
        Makensis = $makensis
        Candle = $candle
        Light = $light
    }
}

function Assert-WindowsInstallerTools {
    param([switch]$RequireInstallers)

    $tools = Get-WindowsInstallerTools

    Write-Host "==> Windows installer tool discovery"
    if ($tools.Makensis) {
        Write-Host "    makensis: $($tools.Makensis)"
    } else {
        Write-Host "    makensis: missing"
    }

    if ($tools.Candle) {
        Write-Host "    candle: $($tools.Candle)"
    } else {
        Write-Host "    candle: missing"
    }

    if ($tools.Light) {
        Write-Host "    light: $($tools.Light)"
    } else {
        Write-Host "    light: missing"
    }

    if ($RequireInstallers -and -not $tools.Makensis) {
        throw "NSIS is required when installer validation is enabled"
    }

    if ($RequireInstallers -and (-not $tools.Candle -or -not $tools.Light)) {
        throw "WiX Toolset is required when installer validation is enabled"
    }

    return $tools
}
