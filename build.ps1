# parcels build script — needs Rust (cargo) and Go 1.24+.
#   .\build.ps1            builds into .\dist
#   .\build.ps1 -Install   also copies to %LOCALAPPDATA%\parcels\bin
param([switch]$Install)
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
New-Item -ItemType Directory -Force "$root\dist" | Out-Null

Write-Host "󰏖 parcels" -ForegroundColor Green
Write-Host "==> engine (rust)" -ForegroundColor Green
Push-Location "$root\core"
cargo build --release
if ($LASTEXITCODE) { throw "cargo build failed" }
Pop-Location

Write-Host "==> tui (go + charm)" -ForegroundColor Green
Push-Location "$root\tui"
go mod tidy
go build -ldflags "-s -w" -o "$root\dist\parcels.exe" .
if ($LASTEXITCODE) { throw "go build failed" }
Pop-Location

Copy-Item "$root\core\target\release\parcels-core.exe" "$root\dist\" -Force
Write-Host "built: $root\dist\parcels.exe (+ parcels-core.exe next to it)" -ForegroundColor Green

if ($Install) {
    $bin = "$env:LOCALAPPDATA\parcels\bin"
    New-Item -ItemType Directory -Force $bin | Out-Null
    Copy-Item "$root\dist\*.exe" $bin -Force
    $path = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($path -notlike "*$bin*") {
        [Environment]::SetEnvironmentVariable("Path", "$path;$bin", "User")
        Write-Host "added $bin to your user PATH (open a new terminal)" -ForegroundColor Yellow
    }
}
