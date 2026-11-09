param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$ArtifactPath,
    [Parameter(Mandatory = $true)]
    [string]$SignaturePath,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^https://')]
    [string]$ArtifactUrl,
    [Parameter(Mandatory = $true)]
    [string]$OutputPath,
    [string]$Notes = 'Stability, security, and routing improvements.'
)

$ErrorActionPreference = 'Stop'
$artifact = [System.IO.Path]::GetFullPath($ArtifactPath)
$signatureFile = [System.IO.Path]::GetFullPath($SignaturePath)
$output = [System.IO.Path]::GetFullPath($OutputPath)
foreach ($required in @($artifact, $signatureFile)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing updater input: $required" }
}
$uri = [Uri]$ArtifactUrl
if ($uri.Scheme -ne 'https' -or -not $uri.Host -or $uri.UserInfo -or $uri.Fragment) {
    throw 'ArtifactUrl must be credential-free HTTPS without a fragment.'
}
$signature = (Get-Content -LiteralPath $signatureFile -Raw -Encoding utf8).Trim()
if (-not $signature) { throw 'Updater signature is empty.' }
$manifest = [ordered]@{
    version = $Version
    notes = $Notes
    pub_date = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = $signature
            url = $uri.AbsoluteUri
        }
    }
}
$parent = Split-Path -Parent $output
if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
[System.IO.File]::WriteAllText($output, ($manifest | ConvertTo-Json -Depth 6), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "UPDATE_FEED_CREATED=$output"
Write-Host "UPDATE_ARTIFACT_SHA256=$((Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash)"
