param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,
    [string]$OutputRoot,
    [ValidatePattern('^https://')]
    [string]$UpdateBaseUrl
)

$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
if (-not $OutputRoot) {
    $OutputRoot = Join-Path $project 'release'
}
$outputRootPath = [System.IO.Path]::GetFullPath($OutputRoot)
$release = Join-Path $outputRootPath $Version
if (Test-Path -LiteralPath $release) {
    throw "Release path already exists; package into a new version or output root: $release"
}

$target = Join-Path $project 'src-tauri\target\release'
$setupSource = Join-Path $target "bundle\nsis\AI Route Manager_${Version}_x64-setup.exe"
$setupSignatureSource = "$setupSource.sig"
$appSource = Join-Path $target 'ai-route-manager.exe'
$loaderSource = Join-Path $target 'WebView2Loader.dll'
$engineSource = Join-Path $project 'src-tauri\resources\engine'
foreach ($required in @($setupSource, $setupSignatureSource, $appSource, $loaderSource, (Join-Path $engineSource 'DynamicProxyEngine.exe'))) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required release input is missing: $required"
    }
}

$portableName = "AI-Route-Manager-$Version-Windows-x64-portable"
$portable = Join-Path $release $portableName
$engineDestination = Join-Path $portable 'resources\engine'
New-Item -ItemType Directory -Path $engineDestination -Force | Out-Null

$setupName = "AI-Route-Manager-$Version-Windows-x64-setup.exe"
Copy-Item -LiteralPath $setupSource -Destination (Join-Path $release $setupName)
$signatureName = "$setupName.sig"
Copy-Item -LiteralPath $setupSignatureSource -Destination (Join-Path $release $signatureName)
Copy-Item -LiteralPath $appSource -Destination (Join-Path $portable 'AI Route Manager.exe')
Copy-Item -LiteralPath $loaderSource -Destination (Join-Path $portable 'WebView2Loader.dll')
Copy-Item -Path (Join-Path $engineSource '*') -Destination $engineDestination -Recurse

$zipName = "$portableName.zip"
$zipPath = Join-Path $release $zipName
Compress-Archive -LiteralPath $portable -DestinationPath $zipPath -CompressionLevel Optimal

$deliverables = @(
    (Join-Path $release $setupName),
    (Join-Path $release $signatureName),
    $zipPath
)
$manifestPath = $null
if ($UpdateBaseUrl) {
    $updateBaseUri = [Uri]$UpdateBaseUrl
    $expectedReleaseSuffix = "/releases/$Version"
    if ($updateBaseUri.Scheme -ne 'https' -or $updateBaseUri.UserInfo -or $updateBaseUri.Fragment -or
        -not $updateBaseUri.AbsolutePath.TrimEnd('/').EndsWith($expectedReleaseSuffix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "UpdateBaseUrl must be credential-free HTTPS and end with $expectedReleaseSuffix."
    }
    $manifestPath = Join-Path $release 'latest.json'
    $artifactUrl = $UpdateBaseUrl.TrimEnd('/') + '/' + $setupName
    & (Join-Path $PSScriptRoot 'generate-update-feed.ps1') -Version $Version -ArtifactPath (Join-Path $release $setupName) -SignaturePath (Join-Path $release $signatureName) -ArtifactUrl $artifactUrl -OutputPath $manifestPath
    if (-not $?) { throw 'Update feed generation failed' }
    $deliverables += $manifestPath
}
$hashes = foreach ($file in $deliverables) {
    '{0}  {1}' -f (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash, (Split-Path $file -Leaf)
}
Set-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt') -Value $hashes -Encoding utf8

if (-not (Test-Path -LiteralPath (Join-Path $portable 'resources\engine\DynamicProxyEngine.exe'))) {
    throw 'Portable package is missing resources\engine\DynamicProxyEngine.exe'
}
if (Test-Path -LiteralPath (Join-Path $portable 'resources\DynamicProxyEngine.exe')) {
    throw 'Portable package contains a flattened engine directory.'
}

Write-Host "WINDOWS_PACKAGE_OK=$release"
if ($manifestPath) { Write-Host "SIGNED_UPDATE_FEED=$manifestPath" } else { Write-Host 'SIGNED_UPDATE_FEED=SKIPPED (no -UpdateBaseUrl)' }
Get-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt')
