$ErrorActionPreference = 'Stop'

Write-Host "SPP 2.0.0 Beta installer"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust/Cargo not found. Install the Rust toolchain first."
    exit 1
}

cargo build --release

$bin = Join-Path $env:LOCALAPPDATA 'SPP\bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
Copy-Item 'target\release\spp.exe' (Join-Path $bin 'spp.exe') -Force

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
if (($userPath -split ';') -notcontains $bin) {
    $newPath = if ($userPath.Trim()) { "$userPath;$bin" } else { $bin }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
}

Write-Host ""
Write-Host "SPP installed: $bin\spp.exe"
Write-Host "Open a new PowerShell/VS Code terminal and run:"
Write-Host "  spp --version"
Write-Host "  spp run .\examples\hello.spp"
