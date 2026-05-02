param(
    [switch]$Restart
)

$ErrorActionPreference = "Stop"

$CoreDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$WorkspaceDir = Resolve-Path (Join-Path $CoreDir "..")
$EnvPath = Join-Path $CoreDir ".env"
$LogDir = Join-Path $CoreDir "target\dev-logs"
$AddonRepos = @{
    "typenx-addon-myanimelist" = "https://github.com/typenx/typenx-addon-myanimelist.git"
    "typenx-addon-anilist" = "https://github.com/typenx/typenx-addon-anilist.git"
    "typenx-addon-kitsu" = "https://github.com/typenx/typenx-addon-kitsu.git"
    "typenx-addon-video-library" = "https://github.com/typenx/typenx-addon-video-library.git"
    "typenx-addon-nxvideo" = "https://github.com/typenx/typenx-addon-nxvideo.git"
    "typenx-addon-plex" = "https://github.com/typenx/typenx-addon-plex.git"
    "typenx-addon-jellyfin" = "https://github.com/typenx/typenx-addon-jellyfin.git"
}

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

function Find-AddonDirectory {
    param([string]$Name)

    $workspacePath = Join-Path $WorkspaceDir $Name
    if (Test-Path $workspacePath) {
        return (Resolve-Path $workspacePath).Path
    }

    $userProfile = [Environment]::GetFolderPath("UserProfile")
    if (-not $userProfile) {
        return $null
    }

    Write-Host "Searching $userProfile for $Name..."
    $match = Get-ChildItem `
        -Path $userProfile `
        -Directory `
        -Filter $Name `
        -Recurse `
        -Force `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1

    if ($match) {
        return $match.FullName
    }

    return $null
}

function Ensure-AddonDirectory {
    param([string]$Name)

    $addonDir = Find-AddonDirectory -Name $Name
    if ($addonDir) {
        Write-Host "Using $Name at $addonDir"
    } else {
        $repoUrl = $AddonRepos[$Name]
        if (-not $repoUrl) {
            throw "No clone URL configured for $Name."
        }

        $addonDir = Join-Path $WorkspaceDir $Name
        Write-Host "$Name was not found under the user directory. Cloning $repoUrl to $addonDir..."
        git clone $repoUrl $addonDir
    }

    $packageJson = Join-Path $addonDir "package.json"
    $nodeModules = Join-Path $addonDir "node_modules"
    if ((Test-Path $packageJson) -and -not (Test-Path $nodeModules)) {
        Write-Host "Installing dependencies for $Name..."
        Push-Location $addonDir
        try {
            npm install
        }
        finally {
            Pop-Location
        }
    }

    return $addonDir
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
    8080, 8787, 8788, 8789, 8791, 8792, 8793, 8794 | ForEach-Object { Stop-PortListener -Port $_ }
}

if (-not $env:MAL_CLIENT_ID) {
    throw "MAL_CLIENT_ID is missing. Add it to core\.env before starting the backend stack."
}

$services = @()

try {
    $myAnimeListAddonDir = Ensure-AddonDirectory -Name "typenx-addon-myanimelist"
    $aniListAddonDir = Ensure-AddonDirectory -Name "typenx-addon-anilist"
    $kitsuAddonDir = Ensure-AddonDirectory -Name "typenx-addon-kitsu"
    $videoLibraryAddonDir = Ensure-AddonDirectory -Name "typenx-addon-video-library"
    $nxVideoAddonDir = Ensure-AddonDirectory -Name "typenx-addon-nxvideo"
    $plexAddonDir = Ensure-AddonDirectory -Name "typenx-addon-plex"
    $jellyfinAddonDir = Ensure-AddonDirectory -Name "typenx-addon-jellyfin"

    $services += Start-ServiceProcess `
        -Name "typenx-addon-myanimelist" `
        -WorkingDirectory $myAnimeListAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8787" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-anilist" `
        -WorkingDirectory $aniListAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8788" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-kitsu" `
        -WorkingDirectory $kitsuAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8789" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-video-library" `
        -WorkingDirectory $videoLibraryAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8791" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-nxvideo" `
        -WorkingDirectory $nxVideoAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8792" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-plex" `
        -WorkingDirectory $plexAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8793" }

    $services += Start-ServiceProcess `
        -Name "typenx-addon-jellyfin" `
        -WorkingDirectory $jellyfinAddonDir `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev") `
        -Environment @{ PORT = "8794" }

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
    Write-Host "  Video lib:   http://127.0.0.1:8791/manifest"
    Write-Host "  NXVideo:     http://127.0.0.1:8792/manifest"
    Write-Host "  Plex:        http://127.0.0.1:8793/manifest"
    Write-Host "  Jellyfin:    http://127.0.0.1:8794/manifest"
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
    8080, 8787, 8788, 8789, 8791, 8792, 8793, 8794 | ForEach-Object { Stop-PortListener -Port $_ }
}
