param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,
    [Parameter(Mandatory = $true)]
    [string]$ArtifactPath,
    [Parameter(Mandatory = $true)]
    [string]$SignaturePath,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$ExpectedVersion
)

$ErrorActionPreference = 'Stop'
$manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding utf8 | ConvertFrom-Json
$platform = $manifest.platforms.'windows-x86_64'
if ($manifest.version -ne $ExpectedVersion) { throw "Manifest version mismatch: $($manifest.version)" }
if (-not $platform) { throw 'Manifest is missing windows-x86_64.' }
$uri = [Uri]$platform.url
if ($uri.Scheme -ne 'https' -or -not $uri.Host -or $uri.UserInfo -or $uri.Fragment) { throw 'Manifest artifact URL is not safe HTTPS.' }
$signature = (Get-Content -LiteralPath $SignaturePath -Raw -Encoding utf8).Trim()
if ($platform.signature -ne $signature) { throw 'Manifest signature does not match the Tauri-generated signature file.' }
if (-not (Test-Path -LiteralPath $ArtifactPath -PathType Leaf)) { throw 'Update artifact is missing.' }
if ((Get-Item -LiteralPath $ArtifactPath).Length -le 0) { throw 'Update artifact is empty.' }
Write-Host "UPDATE_FEED_VALID=True"
Write-Host "UPDATE_VERSION=$($manifest.version)"
Write-Host "UPDATE_PLATFORM=windows-x86_64"
Write-Host "UPDATE_URL=$($platform.url)"
Write-Host "UPDATE_SIGNATURE_MATCH=True"
Write-Host "UPDATE_ARTIFACT_SHA256=$((Get-FileHash -LiteralPath $ArtifactPath -Algorithm SHA256).Hash)"
