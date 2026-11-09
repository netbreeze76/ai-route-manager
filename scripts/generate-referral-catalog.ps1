param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9._-]{1,80}$')]
    [string]$CatalogVersion,
    [Parameter(Mandatory = $true)]
    [string]$OffersPath,
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$offersFile = (Resolve-Path -LiteralPath $OffersPath).Path
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $project 'release\referrals' }
$output = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Path $output -Force | Out-Null

$offers = @(Get-Content -LiteralPath $offersFile -Raw -Encoding utf8 | ConvertFrom-Json)
if ($offers.Count -gt 100) { throw 'Referral catalog cannot contain more than 100 offers.' }
$ids = @{}
$normalizedOffers = @()
$allowedCategories = @('vps', 'vpn', 'hardware', 'software')
$allowedPlacements = @('vps', 'stable-vpn', 'download-vpn', 'remote-control', 'general')
foreach ($offer in $offers) {
    $id = [string]$offer.id
    $category = [string]$offer.category
    $placement = [string]$offer.placement
    $provider = [string]$offer.provider
    $title = [string]$offer.title
    $url = [string]$offer.url
    $disclosure = [string]$offer.disclosure
    $priority = if ($null -eq $offer.priority) { 50 } else { [int]$offer.priority }
    if ($id -notmatch '^publisher-[A-Za-z0-9._-]+$') { throw "Publisher offer id is invalid: $id" }
    if ($ids.ContainsKey($id)) { throw "Duplicate publisher offer id: $id" }
    $ids[$id] = $true
    if ($category -notin $allowedCategories) { throw "Publisher offer category is invalid: $id" }
    if ($placement -notin $allowedPlacements) { throw "Publisher offer placement is invalid: $id" }
    if ([string]::IsNullOrWhiteSpace($provider) -or [string]::IsNullOrWhiteSpace($title)) {
        throw "Publisher offer provider and title are required: $id"
    }
    if ($priority -lt 0 -or $priority -gt 100) { throw "Publisher offer priority must be between 0 and 100: $id" }
    if ([bool]$offer.enabled) {
        $uri = [Uri]$url
        if ($uri.Scheme -ne 'https' -or -not $uri.Host -or $uri.UserInfo -or $uri.Fragment) {
            throw "Enabled publisher offer must use safe HTTPS: $id"
        }
    }
    if (-not ($disclosure.Contains('推广') -or $disclosure.Contains('返利'))) {
        throw "Publisher offer must disclose referral relationship: $id"
    }
    $normalizedOffers += [ordered]@{
        id = $id
        category = $category
        placement = $placement
        provider = $provider
        title = $title
        summary = if ([string]::IsNullOrWhiteSpace([string]$offer.summary)) { $title } else { [string]$offer.summary }
        badge = if ([string]::IsNullOrWhiteSpace([string]$offer.badge)) { '推荐' } else { [string]$offer.badge }
        price = if ([string]::IsNullOrWhiteSpace([string]$offer.price)) { '以服务商页面为准' } else { [string]$offer.price }
        url = $url
        coupon = [string]$offer.coupon
        disclosure = $disclosure
        enabled = [bool]$offer.enabled
        priority = $priority
        clickCount = 0
    }
}

$payloadObject = [ordered]@{
    catalogVersion = $CatalogVersion
    generatedAt = [DateTimeOffset]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ssZ')
    offers = $normalizedOffers
}
$payloadPath = Join-Path $output 'payload.json'
$payloadJson = $payloadObject | ConvertTo-Json -Depth 20 -Compress
[IO.File]::WriteAllText($payloadPath, $payloadJson, [Text.UTF8Encoding]::new($false))

$localKey = Join-Path $env:USERPROFILE '.tauri\ai-route-manager-updater.key'
$protectedPassword = Join-Path $env:USERPROFILE '.tauri\ai-route-manager-updater.password.dpapi'
foreach ($required in @($localKey, $protectedPassword)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Signing input is missing: $required" }
}

Add-Type -AssemblyName System.Security
$ciphertext = [Convert]::FromBase64String((Get-Content -LiteralPath $protectedPassword -Raw).Trim())
$entropy = [Text.Encoding]::UTF8.GetBytes('AI Route Manager updater signing password v1')
$plaintext = [System.Security.Cryptography.ProtectedData]::Unprotect($ciphertext, $entropy, [System.Security.Cryptography.DataProtectionScope]::CurrentUser)
$previousPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Text.Encoding]::UTF8.GetString($plaintext)
    Push-Location $project
    try {
        & pnpm tauri signer sign --private-key-path $localKey $payloadPath
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    finally { Pop-Location }
}
finally {
    if ($null -eq $previousPassword) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue }
    else { $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPassword }
    [Array]::Clear($plaintext, 0, $plaintext.Length)
}

$signaturePath = "$payloadPath.sig"
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) { throw 'Tauri signer did not create payload.json.sig.' }
$signature = (Get-Content -LiteralPath $signaturePath -Raw -Encoding utf8).Trim()
if (-not $signature) { throw 'Referral catalog signature is empty.' }
$payloadBytes = [IO.File]::ReadAllBytes($payloadPath)
$envelope = [ordered]@{
    schemaVersion = 1
    payload = [Convert]::ToBase64String($payloadBytes)
    signature = $signature
}
$latestPath = Join-Path $output 'latest.json'
$latestJson = $envelope | ConvertTo-Json -Depth 5 -Compress
[IO.File]::WriteAllText($latestPath, $latestJson, [Text.UTF8Encoding]::new($false))

Write-Host "REFERRAL_CATALOG_CREATED=$latestPath"
Write-Host "REFERRAL_CATALOG_VERSION=$CatalogVersion"
Write-Host "REFERRAL_CATALOG_OFFERS=$($offers.Count)"
Write-Host "REFERRAL_PAYLOAD_SHA256=$((Get-FileHash -LiteralPath $payloadPath -Algorithm SHA256).Hash)"
Write-Host "REFERRAL_ENVELOPE_SHA256=$((Get-FileHash -LiteralPath $latestPath -Algorithm SHA256).Hash)"
