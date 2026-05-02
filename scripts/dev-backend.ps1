param(
    [switch]$Restart
)

$ErrorActionPreference = "Stop"

$CoreDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$WorkspaceDir = Resolve-Path (Join-Path $CoreDir "..")
$EnvPath = Join-Path $CoreDir ".env"
$LogDir = Join-Path $CoreDir "target\dev-logs"

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function Import-DotEnv {
    param([string]$Path)

    if (-not (Test-Path $Path)) {
        Write-Warning "No .env found at $Path. Continuing with existing shell environment."
        return
    }

    Get-Content $Path | ForEach-Object {
        $line = $_.Trim()
        if (-not $line -or $line.StartsWith("#")) {
            return
        }

        $parts = $line.Split("=", 2)
        if ($parts.Count -ne 2) {
            return
        }

        $name = $parts[0].Trim()
        $value = $parts[1].Trim().Trim('"').Trim("'")
        [Environment]::SetEnvironmentVariable($name, $value, "Process")
    }
}

function Stop-PortListener {
    param([int]$Port)

    $listeners = @(
        netstat -ano |
            ForEach-Object {
                $columns = ($_ -replace "^\s+", "") -split "\s+"
                if ($columns.Count -ge 5 -and $columns[3] -eq "LISTENING") {
                    $localAddress = $columns[1]
                    if ($localAddress -match "[:.]$Port$") {
                        $columns[4]
                    }
                }
            } |
            Where-Object { $_ } |
            Sort-Object -Unique
    )

    foreach ($processId in $listeners) {
        if ($processId -and $processId -ne "0") {
            Write-Host "Stopping existing process on port $Port (PID $processId)"
            Stop-Process -Id ([int]$processId) -Force -ErrorAction SilentlyContinue
        }
    }
}

function Start-ServiceProcess {
    param(
        [string]$Name,
        [string]$WorkingDirectory,
        [string]$FilePath,
        [string[]]$ArgumentList,
        [hashtable]$Environment = @{}
    )

    foreach ($key in $Environment.Keys) {
        [Environment]::SetEnvironmentVariable($key, [string]$Environment[$key], "Process")
    }

    $stdout = Join-Path $LogDir "$Name.log"
    $stderr = Join-Path $LogDir "$Name.err.log"
    if (Test-Path $stdout) { Remove-Item $stdout -Force }
    if (Test-Path $stderr) { Remove-Item $stderr -Force }

    Write-Host "Starting $Name..."
    $process = Start-Process `
        -FilePath $FilePath `
        -ArgumentList $ArgumentList `
        -WorkingDirectory $WorkingDirectory `
        -RedirectStandardOutput $stdout `
        -RedirectStandardError $stderr `
        -WindowStyle Hidden `
        -PassThru

    [pscustomobject]@{
        Name = $Name
        Process = $process
        Stdout = $stdout
        Stderr = $stderr
    }
}

Import-DotEnv -Path $EnvPath

if ($Restart) {
    8080, 8787, 8788, 8789 | ForEach-Object { Stop-PortListener -Port $_ }
}

if (-not $env:MAL_CLIENT_ID) {
    throw "MAL_CLIENT_ID is missing. Add it to core\.env before starting the backend stack."
}

$services = @()

try {
    $services += Start-ServiceProcess `
        -Name "typenx-addon-myanimelist" `
        -WorkingDirectory (Join-Path $WorkspaceDir "typenx-addon-myanimelist") `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8787" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-anilist" `
        -WorkingDirectory (Join-Path $WorkspaceDir "typenx-addon-anilist") `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8788" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-kitsu" `
        -WorkingDirectory (Join-Path $WorkspaceDir "typenx-addon-kitsu") `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8789" }

    $services += Start-ServiceProcess `
        -Name "typenx-server" `
        -WorkingDirectory $CoreDir `
        -FilePath "cargo" `
        -ArgumentList @("run", "-p", "typenx-server")

    Write-Host ""
    Write-Host "Typenx backend stack is starting:"
    Write-Host "  Core:        http://127.0.0.1:8080/health"
    Write-Host "  MAL addon:   http://127.0.0.1:8787/manifest"
    Write-Host "  AniList:     http://127.0.0.1:8788/manifest"
    Write-Host "  Kitsu:       http://127.0.0.1:8789/manifest"
    Write-Host ""
    Write-Host "Logs are in $LogDir"
    Write-Host "Press Ctrl+C to stop the backend stack."

    while ($true) {
        foreach ($service in $services) {
            if ($service.Process.HasExited) {
                $errorText = if (Test-Path $service.Stderr) {
                    Get-Content $service.Stderr -Raw
                } else {
                    ""
                }
                throw "$($service.Name) exited with code $($service.Process.ExitCode). $errorText"
            }
        }

        Start-Sleep -Seconds 2
    }
}
finally {
    Write-Host ""
    Write-Host "Stopping Typenx backend stack..."
    foreach ($service in $services) {
        if ($service.Process -and -not $service.Process.HasExited) {
            Stop-Process -Id $service.Process.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Start-Sleep -Milliseconds 500
    8080, 8787, 8788, 8789 | ForEach-Object { Stop-PortListener -Port $_ }
}
