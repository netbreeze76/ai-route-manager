param(
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,
    [ValidatePattern('^https://')]
    [string]$UpdateBaseUrl
)

$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
if (-not $Version) {
    $cargoText = Get-Content -LiteralPath (Join-Path $project 'src-tauri\Cargo.toml') -Raw
    $match = [regex]::Match($cargoText, '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"')
    if (-not $match.Success) { throw 'Could not read package version from Cargo.toml' }
    $Version = $match.Groups[1].Value
}
& (Join-Path $PSScriptRoot 'test-powershell.ps1')
if (-not $?) { exit 1 }
& (Join-Path $PSScriptRoot 'check-runtime-footprint.ps1')
if (-not $?) { exit 1 }
& (Join-Path $PSScriptRoot 'check-home-gateway-policy.ps1')
if (-not $?) { exit 1 }

& (Join-Path $PSScriptRoot 'build-engine.ps1')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& (Join-Path $PSScriptRoot 'test-rust-gnu.ps1')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$workspace = Split-Path $project -Parent
$rustBin = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin'
$llvmBin = Join-Path $workspace 'work\llvm-mingw\toolchain-msvcrt-20261020\llvm-mingw-20261020-msvcrt-x86_64\bin'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$rustBin;$llvmBin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:RUSTFLAGS = '-C linker=rust-lld.exe -C link-self-contained=yes'
$env:CC = 'x86_64-w64-mingw32-gcc.exe'
$env:CXX = 'x86_64-w64-mingw32-g++.exe'
$env:AR = 'llvm-ar.exe'

if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
    $localKey = Join-Path $env:USERPROFILE '.tauri\ai-route-manager-updater.key'
    $protectedPassword = Join-Path $env:USERPROFILE '.tauri\ai-route-manager-updater.password.dpapi'
    if (-not (Test-Path -LiteralPath $localKey) -or -not (Test-Path -LiteralPath $protectedPassword)) {
        throw 'Updater signing credentials are missing. Set TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD, or provision the protected local key.'
    }
    Add-Type -AssemblyName System.Security
    $ciphertext = [Convert]::FromBase64String((Get-Content -LiteralPath $protectedPassword -Raw).Trim())
    $entropy = [Text.Encoding]::UTF8.GetBytes('AI Route Manager updater signing password v1')
    $plaintext = [System.Security.Cryptography.ProtectedData]::Unprotect($ciphertext, $entropy, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
    $env:TAURI_SIGNING_PRIVATE_KEY = $localKey
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Text.Encoding]::UTF8.GetString($plaintext)
}

Push-Location $project
try {
    & pnpm check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & pnpm tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}

$releaseRoot = [System.IO.Path]::GetFullPath((Join-Path $project 'release'))
$stagingRoot = Join-Path $releaseRoot ('.staging-' + [guid]::NewGuid().ToString('N'))
$destination = Join-Path $releaseRoot $Version
$backup = Join-Path $releaseRoot ('.backup-' + $Version + '-' + [guid]::NewGuid().ToString('N'))
$releasePrefix = $releaseRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
foreach ($managedPath in @($stagingRoot, $destination, $backup)) {
    $resolvedManagedPath = [System.IO.Path]::GetFullPath($managedPath)
    if (-not $resolvedManagedPath.StartsWith($releasePrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing release operation outside release root: $resolvedManagedPath"
    }
}
try {
    $packageArgs = @{ Version = $Version; OutputRoot = $stagingRoot }
    if ($UpdateBaseUrl) { $packageArgs.UpdateBaseUrl = $UpdateBaseUrl }
    & (Join-Path $PSScriptRoot 'package-windows.ps1') @packageArgs
    if (-not $?) { throw 'Windows package step failed' }
    $stagedRelease = Join-Path $stagingRoot $Version
    if (-not (Test-Path -LiteralPath (Join-Path $stagedRelease 'SHA256SUMS.txt'))) {
        throw 'Staged release is missing SHA256SUMS.txt'
    }
    if (Test-Path -LiteralPath $destination) {
        Move-Item -LiteralPath $destination -Destination $backup
    }
    try {
        Move-Item -LiteralPath $stagedRelease -Destination $destination
    }
    catch {
        if ((Test-Path -LiteralPath $backup) -and -not (Test-Path -LiteralPath $destination)) {
            Move-Item -LiteralPath $backup -Destination $destination
        }
        throw
    }
    if (Test-Path -LiteralPath $backup) {
        Remove-Item -LiteralPath $backup -Recurse -Force
    }
}
finally {
    if (Test-Path -LiteralPath $stagingRoot) {
        Remove-Item -LiteralPath $stagingRoot -Recurse -Force
    }
}

Write-Host "WINDOWS_RELEASE_PUBLISHED=$destination"
Get-Content -LiteralPath (Join-Path $destination 'SHA256SUMS.txt')
