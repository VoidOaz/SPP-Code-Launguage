[CmdletBinding()]
param(
    [switch]$Clean,
    [switch]$SkipChecks
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Root = (Resolve-Path $PSScriptRoot).Path
$ExtDir = $Root
$DistDir = Join-Path $Root 'dist'
$TargetExe = Join-Path $Root 'target\release\spp.exe'
$BundledExe = Join-Path $ExtDir 'bin\spp.exe'
$VsixOut = Join-Path $DistDir 'SPP-2.0.0.vsix'

function Info($m) { Write-Host "[SPP] $m" -ForegroundColor Cyan }
function Good($m) { Write-Host "[OK]  $m" -ForegroundColor Green }
function Fail($m) { throw "[SPP] $m" }

function Test-Command($name) {
    return $null -ne (Get-Command $name -ErrorAction SilentlyContinue)
}

function Import-VsDevEnvironment {
    $vswhereCandidates = @(
        "$env:ProgramFiles\Microsoft Visual Studio\Installer\vswhere.exe",
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    )
    $vswhere = $vswhereCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

    if (-not $vswhere) {
        $vsdev = Get-ChildItem "${env:ProgramFiles}\Microsoft Visual Studio" -Filter VsDevCmd.bat -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $vsdev) {
            $vsdev = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft Visual Studio" -Filter VsDevCmd.bat -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        }
    } else {
        $install = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null | Select-Object -First 1)
        if ($install) { $vsdev = Join-Path $install 'Common7\Tools\VsDevCmd.bat' }
    }

    if (-not $vsdev -or -not (Test-Path $vsdev)) {
        Fail "Visual Studio C++ Build Tools bulunamadı. Visual Studio Build Tools kurup 'Desktop development with C++' workload'unu seç."
    }

    Info "MSVC ortamı hazırlanıyor..."
    $cmd = '"{0}" -arch=x64 -host_arch=x64 >nul && set' -f $vsdev
    $lines = cmd.exe /d /s /c $cmd
    foreach ($line in $lines) {
        if ($line -match '^(.*?)=(.*)$') {
            $name = $Matches[1]
            $value = $Matches[2]
            if ($name -and $name -notmatch '^=') { Set-Item "Env:$name" $value }
        }
    }

    if (-not (Test-Command 'cl.exe')) { Fail 'MSVC ortamı yüklendi fakat cl.exe bulunamadı.' }
    if (-not (Test-Command 'lib.exe')) { Fail 'MSVC ortamı yüklendi fakat lib.exe bulunamadı.' }
    Good 'MSVC C++ compiler hazır.'
}

Info 'SPP 2.0.0 release build başlatılıyor.'
Info "Proje: $Root"

if (-not (Test-Command 'cargo')) {
    Fail 'Rust/Cargo bulunamadı. Rustup kurulduktan sonra yeni PowerShell açıp tekrar çalıştır.'
}
Good "Cargo: $(& cargo --version)"

if (-not (Test-Command 'rustc')) { Fail 'rustc bulunamadı. Rust kurulumunu kontrol et.' }

if (-not (Test-Command 'cl.exe')) { Import-VsDevEnvironment }
else { Good 'MSVC C++ compiler zaten PATH üzerinde.' }

if ($Clean) {
    Info 'Cargo clean çalışıyor...'
    cargo clean --manifest-path (Join-Path $Root 'Cargo.toml')
}

Info 'Rust + C++ native backend release build ediliyor...'
Push-Location $Root
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { Fail 'cargo build başarısız oldu.' }
} finally { Pop-Location }

if (-not (Test-Path $TargetExe)) { Fail "Build tamamlandı fakat spp.exe bulunamadı: $TargetExe" }
Good "Compiler hazır: $TargetExe"

Info 'SPP runtime smoke test...'
$version = & $TargetExe --version 2>&1
if ($LASTEXITCODE -ne 0) { Fail "spp.exe --version başarısız: $version" }
Good "Runtime: $version"

if (-not $SkipChecks) {
    $example = Join-Path $Root 'examples\hello.spp'
    if (Test-Path $example) {
        Info 'Örnek program kontrol ediliyor...'
        & $TargetExe check $example
        if ($LASTEXITCODE -ne 0) { Fail 'examples/hello.spp check başarısız.' }
        Good 'SPP örnek check geçti.'
    }
}

Info 'Compiler VS Code extension içine kopyalanıyor...'
New-Item -ItemType Directory -Force -Path (Split-Path $BundledExe) | Out-Null
Copy-Item -Force $TargetExe $BundledExe
Good "Bundled runtime: $BundledExe"

Info 'VSIX paketleyici kontrol ediliyor...'
if (-not (Test-Command 'npx')) { Fail 'Node.js/npx bulunamadı. Node.js LTS kur ve tekrar çalıştır.' }

New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
Remove-Item -Force -ErrorAction SilentlyContinue $VsixOut

Push-Location $ExtDir
try {
    Info 'VSIX oluşturuluyor...'
    & npx --yes @vscode/vsce package --out $VsixOut --no-dependencies
    if ($LASTEXITCODE -ne 0) { Fail 'vsce package başarısız oldu.' }
} finally { Pop-Location }

if (-not (Test-Path $VsixOut)) { Fail "VSIX oluşmadı: $VsixOut" }
$size = [math]::Round((Get-Item $VsixOut).Length / 1KB, 1)
Good "VSIX hazır: $VsixOut ($size KB)"

Info 'Paket içeriği kontrol ediliyor...'
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [System.IO.Compression.ZipFile]::OpenRead($VsixOut)
try {
    $names = $zip.Entries.FullName
    if ($names -notcontains 'extension/bin/spp.exe') {
        Fail 'VSIX içinde extension/bin/spp.exe bulunamadı; paket eksik.'
    }
    Good 'VSIX içinde spp.exe mevcut.'
} finally { $zip.Dispose() }

Write-Host ''
Write-Host '========================================' -ForegroundColor Green
Write-Host ' SPP 2.0.0 VSIX HAZIR 🚀' -ForegroundColor Green
Write-Host " $VsixOut" -ForegroundColor White
Write-Host '========================================' -ForegroundColor Green
