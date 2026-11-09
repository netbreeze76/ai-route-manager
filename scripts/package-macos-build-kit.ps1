param(
    [string]$OutputDirectory = [Environment]::GetFolderPath('Desktop')
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$workspace = Split-Path $project -Parent
$engineSource = Join-Path $workspace 'dynamic_proxy_manager'
$version = (Get-Content -LiteralPath (Join-Path $project 'package.json') -Raw -Encoding utf8 | ConvertFrom-Json).version
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$zipPath = Join-Path $outputRoot "AI-Route-Manager-macOS-build-kit-$version.zip"

& (Join-Path $PSScriptRoot 'test-macos-build-modes.ps1')
if ($LASTEXITCODE -ne 0) { throw 'macOS build mode tests failed' }

if (-not (Test-Path -LiteralPath (Join-Path $engineSource 'dynamic_proxy_manager.py'))) {
    throw "Dynamic proxy manager source is missing: $engineSource"
}
if (Test-Path -LiteralPath $zipPath) {
    throw "Output already exists; move or rename it before creating another kit: $zipPath"
}

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
$tempRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$stage = Join-Path $tempRoot ("ai-route-manager-macos-kit-" + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $stage "AI-Route-Manager-macOS-build-kit-$version"

function Invoke-RobocopyChecked {
    param(
        [Parameter(Mandatory)] [string]$Source,
        [Parameter(Mandatory)] [string]$Destination,
        [string[]]$ExcludeDirectories = @(),
        [string[]]$ExcludeFiles = @()
    )
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    $arguments = @($Source, $Destination, '/E', '/R:1', '/W:1', '/NFL', '/NDL', '/NJH', '/NJS', '/NP')
    if ($ExcludeDirectories.Count -gt 0) { $arguments += '/XD'; $arguments += $ExcludeDirectories }
    if ($ExcludeFiles.Count -gt 0) { $arguments += '/XF'; $arguments += $ExcludeFiles }
    & robocopy @arguments | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "robocopy failed with exit code $LASTEXITCODE" }
}

try {
    $projectDestination = Join-Path $payload 'ai-route-manager'
    Invoke-RobocopyChecked -Source $project -Destination $projectDestination `
        -ExcludeDirectories @(
            (Join-Path $project 'node_modules'),
            (Join-Path $project 'dist'),
            (Join-Path $project 'release'),
            (Join-Path $project 'src-tauri\target'),
            (Join-Path $project 'src-tauri\resources\engine'),
            (Join-Path $project '.git')
        ) `
        -ExcludeFiles @('*.pdb', '*.log', '*.local.json', '*.p12', '*.key')

    $engineDestination = Join-Path $payload 'dynamic_proxy_manager'
    Invoke-RobocopyChecked -Source $engineSource -Destination $engineDestination `
        -ExcludeDirectories @(
            (Join-Path $engineSource '__pycache__'),
            (Join-Path $engineSource '.pytest_cache'),
            (Join-Path $engineSource 'build'),
            (Join-Path $engineSource 'dist'),
            (Join-Path $engineSource 'work'),
            (Join-Path $engineSource '.git')
        ) `
        -ExcludeFiles @('*.pdb', '*.log', '*.local.json', '*.p12', '*.key')

    $readme = @"
# AI Route Manager macOS build kit $version

1. Copy this extracted directory to an Apple Silicon or Intel Mac.
2. Keep `ai-route-manager` and `dynamic_proxy_manager` as sibling directories.
3. Install Xcode Command Line Tools, Rust, Node.js and pnpm.
4. Export the Tauri updater signing key variables described in `ai-route-manager/docs/BUILD_MACOS.md`.
5. For a public Developer ID build, also export a signing identity and complete Apple notarization credentials.
6. Run: `bash ./BUILD.command` for an ad-hoc local test DMG; no updater private key or Apple account is needed.
7. After local acceptance, follow `ai-route-manager/docs/BUILD_MACOS.md` and run `bash ./ai-route-manager/scripts/build-macos.sh --release` for Developer ID signing, notarization and updater artifacts.

The build command does not enable managed routing or change the Mac system proxy. It builds and verifies the DMG, updater artifacts and SHA-256 manifest.
"@
    Set-Content -LiteralPath (Join-Path $payload 'START-HERE.md') -Value $readme -Encoding utf8

    $buildCommand = @'
#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/ai-route-manager"
exec bash ./scripts/build-macos.sh --local-test
'@
    Set-Content -LiteralPath (Join-Path $payload 'BUILD.command') -Value $buildCommand -Encoding utf8NoBOM

    $forbiddenNames = Get-ChildItem -LiteralPath $payload -Recurse -File | Where-Object {
        $_.Name -match '^(id_rsa|id_ed25519)$' -or
        $_.Extension -in @('.p12', '.key') -or
        $_.FullName -match '[\\/]private[\\/]'
    }
    if ($forbiddenNames) { throw "Private credential file entered build kit: $($forbiddenNames[0].FullName)" }

    # Build the markers at runtime so this scanner does not match its own source text.
    $privateKeyMarker = '-----BEGIN ' + 'OPENSSH PRIVATE KEY-----'
    $subscriptionTokenMarker = 'subscribe?' + 'token='
    $textCandidates = Get-ChildItem -LiteralPath $payload -Recurse -File | Where-Object Length -lt 10MB
    $privateKeyContent = $textCandidates | Select-String -SimpleMatch -Pattern $privateKeyMarker -List -ErrorAction SilentlyContinue
    $tokenContent = $textCandidates | Select-String -SimpleMatch -Pattern $subscriptionTokenMarker -ErrorAction SilentlyContinue | Where-Object {
        $_.Line -notmatch 'https?://(www\.)?example\.(com|invalid)/'
    }
    $secretContent = @($privateKeyContent) + @($tokenContent)
    if ($secretContent) { throw "Credential-like content entered build kit: $($secretContent[0].Path)" }

    $manifestPath = Join-Path $payload 'MANIFEST.sha256'
    $manifest = Get-ChildItem -LiteralPath $payload -Recurse -File | Where-Object FullName -ne $manifestPath | Sort-Object FullName | ForEach-Object {
        $relative = $_.FullName.Substring($payload.Length + 1).Replace('\', '/')
        '{0}  {1}' -f (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash, $relative
    }
    Set-Content -LiteralPath $manifestPath -Value $manifest -Encoding utf8NoBOM

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, $zipPath, [IO.Compression.CompressionLevel]::Optimal, $true)
    $zip = Get-Item -LiteralPath $zipPath
    $zipHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $zipPath).Hash
    Write-Output "MAC_BUILD_KIT=$($zip.FullName)"
    Write-Output "MAC_BUILD_KIT_BYTES=$($zip.Length)"
    Write-Output "MAC_BUILD_KIT_SHA256=$zipHash"
    Write-Output "MAC_BUILD_KIT_FILES=$($manifest.Count + 1)"
}
finally {
    if (Test-Path -LiteralPath $stage) {
        $resolvedStage = [IO.Path]::GetFullPath($stage)
        $expectedPrefix = $tempRoot.TrimEnd('\') + '\ai-route-manager-macos-kit-'
        if (-not $resolvedStage.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove unexpected staging path: $resolvedStage"
        }
        Remove-Item -LiteralPath $resolvedStage -Recurse -Force
    }
}
