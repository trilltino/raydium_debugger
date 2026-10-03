# Install the native driver matching the installed Edge build for tauri-driver.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$edge = Join-Path ${env:ProgramFiles(x86)} 'Microsoft/Edge/Application/msedge.exe'
$version = (Get-Item -LiteralPath $edge).VersionInfo.ProductVersion
$directory = Join-Path $workspace 'target/native-driver'
New-Item -ItemType Directory -Force -Path $directory | Out-Null
$archive = Join-Path $directory 'edgedriver.zip'
Invoke-WebRequest -Uri "https://msedgedriver.microsoft.com/$version/edgedriver_win64.zip" -OutFile $archive
Expand-Archive -LiteralPath $archive -DestinationPath $directory -Force
$driver = Join-Path $directory 'msedgedriver.exe'
$installed = & $driver --version
if (-not $installed.Contains($version)) { throw 'Edge WebDriver version does not match Edge' }
Write-Output $installed
