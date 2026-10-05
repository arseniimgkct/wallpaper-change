<#
.SYNOPSIS
    Устанавливает Rust toolchain (rustup + stable-msvc) в систему пользователя.

.DESCRIPTION
    Скрипт идемпотентен: если rustup уже установлен, он лишь обновляет toolchain.
    Требуется MSVC Build Tools (проверяется наличие vcvars64.bat), иначе сборка
    не слинкуется. Ничего не ставится глобально и без прав администратора.

.PARAMETER Toolchain
    Канал toolchain: stable | beta | nightly. По умолчанию stable.

.PARAMETER ForceReinstall
    Удалить существующий rustup перед установкой.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\install-rust.ps1
#>
[CmdletBinding()]
param(
    [ValidateSet('stable', 'beta', 'nightly')]
    [string]$Toolchain = 'stable',
    [switch]$ForceReinstall
)

$ErrorActionPreference = 'Stop'

$rustupInit = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
$targetDir = "$env:USERPROFILE\.cargo"
$tempDir = Join-Path $env:TEMP ('rustup-init-' + [guid]::NewGuid().ToString('N'))

function Write-Step([string]$Message) {
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host "    $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host "    $Message" -ForegroundColor Yellow
}

# --- 1. Проверка MSVC --------------------------------------------------------
Write-Step 'Проверяю наличие MSVC Build Tools'
$vsRoot = "${env:ProgramFiles}\Microsoft Visual Studio\2022"
$vcvars = Get-ChildItem -Path "$vsRoot\*\VC\Auxiliary\Build\vcvars64.bat" -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not $vcvars) {
    Write-Warn 'vcvars64.bat не найден — Rust соберётся, но линковка не сработает.'
    Write-Warn 'Установите «Desktop development with C++» из Visual Studio Installer.'
} else {
    Write-Ok "найден $($vcvars.FullName)"
}

# --- 2. Скачивание rustup-init ----------------------------------------------
Write-Step 'Скачиваю rustup-init.exe'
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
$installer = Join-Path $tempDir 'rustup-init.exe'

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $rustupInit -OutFile $installer -UseBasicParsing
} catch {
    Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
    throw "Не удалось скачать rustup-init.exe: $($_.Exception.Message)"
}

Write-Ok "$([math]::Round((Get-Item $installer).Length / 1MB, 1)) MB"

# --- 3. Установка / обновление ---------------------------------------------
$existing = Test-Path (Join-Path $targetDir 'bin\rustup.exe')

if ($existing -and $ForceReinstall) {
    Write-Step 'Удаляю предыдущий rustup'
    Remove-Item -Recurse -Force $targetDir -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$env:USERPROFILE\.rustup" -ErrorAction SilentlyContinue
    $existing = $false
}

if ($existing) {
    Write-Step "Обновляю существующий toolchain ($Toolchain)"
    & (Join-Path $targetDir 'bin\rustup.exe') set profile minimal
    & (Join-Path $targetDir 'bin\rustup.exe') toolchain install $Toolchain --profile minimal
    & (Join-Path $targetDir 'bin\rustup.exe') default $Toolchain
} else {
    Write-Step "Устанавливаю rustup с toolchain $Toolchain"
    & $installer `
        -y `
        --default-toolchain $Toolchain `
        --profile minimal `
        --default-host x86_64-pc-windows-msvc `
        --no-modify-path
}

$exit = $LASTEXITCODE
Remove-Item -Recurse -Force $tempDir -ErrorAction SilentlyContinue
if ($exit -ne 0) {
    throw "rustup-init завершился с кодом $exit"
}

# --- 4. PATH для текущего и будущих сеансов --------------------------------
Write-Step 'Обновляю PATH'
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$targetDir\bin*") {
    $newPath = if ([string]::IsNullOrWhiteSpace($userPath)) { "$targetDir\bin" } else { "$userPath;$targetDir\bin" }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Ok 'каталог .cargo\bin добавлен в пользовательский PATH'
} else {
    Write-Ok 'каталог .cargo\bin уже в PATH'
}

$env:Path = "$targetDir\bin;$env:Path"

# --- 5. Проверка ------------------------------------------------------------
Write-Step 'Ставлю clippy и rustfmt'
foreach ($component in @('clippy', 'rustfmt')) {
    & (Join-Path $targetDir 'bin\rustup.exe') component add $component --toolchain $Toolchain 2>&1 |
        Out-Null
}

Write-Step 'Проверяю установку'
Write-Ok (& rustc --version)
Write-Ok (& cargo --version)
Write-Ok (& cargo clippy --version)

Write-Host ''
Write-Host 'Rust установлен. Открой новое окно терминала, чтобы подхватить PATH.' -ForegroundColor Cyan