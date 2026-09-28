param(
    [string]$Bind = "127.0.0.1:8787"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

function Get-CommandPath($Name) {
    $command = Get-Command $Name -ErrorAction Stop
    return $command.Source
}

function Join-ProcessArguments([string[]]$Arguments) {
    $quoted = foreach ($argument in $Arguments) {
        if ($argument -match '[\s"]') {
            '"' + ($argument -replace '\\(?=")', '\\' -replace '"', '\"') + '"'
        } else {
            $argument
        }
    }
    return $quoted -join " "
}

function Start-DevProcess($Name, $FileName, [string[]]$Arguments) {
    Write-Host "starting $Name" -ForegroundColor Cyan
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FileName
    $startInfo.Arguments = Join-ProcessArguments $Arguments
    $startInfo.WorkingDirectory = $RepoRoot
    $startInfo.UseShellExecute = $false

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    [void]$process.Start()
    return $process
}

function Stop-ProcessTree($Process, $Name) {
    if ($null -eq $Process -or $Process.HasExited) {
        return
    }

    Write-Host "stopping $Name" -ForegroundColor DarkYellow
    & taskkill.exe /PID $Process.Id /T /F | Out-Null
}

function Get-WatchStamp {
    $paths = @(
        "Cargo.toml",
        "Cargo.lock",
        ".env.local",
        "server/Cargo.toml",
        "src-tauri/Cargo.toml",
        "src",
        "server/src",
        "src-tauri/src"
    )

    $latest = 0L
    foreach ($path in $paths) {
        if (-not (Test-Path $path)) {
            continue
        }
        $item = Get-Item $path
        if ($item.PSIsContainer) {
            $files = Get-ChildItem $item.FullName -Recurse -File -Include *.rs,*.toml,*.json
        } else {
            $files = @($item)
        }
        foreach ($file in $files) {
            if ($file.LastWriteTimeUtc.Ticks -gt $latest) {
                $latest = $file.LastWriteTimeUtc.Ticks
            }
        }
    }
    return $latest
}

$cargo = Get-CommandPath "cargo"
$npm = Get-CommandPath "npm.cmd"

$server = $null
$vite = $null
$stamp = Get-WatchStamp

try {
    $server = Start-DevProcess "Axum server" $cargo @("run", "-p", "raydium-debugger-server", "--", "--bind", $Bind)
    $vite = Start-DevProcess "Vite dev server" $npm @("--prefix", "web", "run", "dev")

    Write-Host ""
    Write-Host "dev app: http://127.0.0.1:5173" -ForegroundColor Green
    Write-Host "api:     http://$Bind" -ForegroundColor Green
    Write-Host "Rust changes restart the server; React changes hot reload through Vite." -ForegroundColor DarkGray
    Write-Host ""

    while ($true) {
        Start-Sleep -Milliseconds 900

        if ($vite.HasExited) {
            exit $vite.ExitCode
        }

        $nextStamp = Get-WatchStamp
        if ($nextStamp -ne $stamp) {
            $stamp = $nextStamp
            Stop-ProcessTree $server "Axum server"
            $server = Start-DevProcess "Axum server" $cargo @("run", "-p", "raydium-debugger-server", "--", "--bind", $Bind)
        } elseif ($server.HasExited) {
            Write-Host "Axum server exited; waiting for a Rust/config change before restarting." -ForegroundColor DarkYellow
            while ($server.HasExited) {
                Start-Sleep -Milliseconds 900
                if ($vite.HasExited) {
                    exit $vite.ExitCode
                }
                $nextStamp = Get-WatchStamp
                if ($nextStamp -ne $stamp) {
                    $stamp = $nextStamp
                    $server = Start-DevProcess "Axum server" $cargo @("run", "-p", "raydium-debugger-server", "--", "--bind", $Bind)
                    break
                }
            }
        }
    }
}
finally {
    Stop-ProcessTree $vite "Vite dev server"
    Stop-ProcessTree $server "Axum server"
}
