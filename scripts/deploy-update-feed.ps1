param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$RemoteHost,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9.-]+$')]
    [string]$Domain,
    [Parameter(Mandatory = $true)]
    [string]$IdentityFile,
    [Parameter(Mandatory = $true)]
    [string]$ReleaseDirectory,
    [string]$ReferralCatalogDirectory,
    [ValidatePattern('^[a-z_][a-z0-9_-]{0,31}$')]
    [string]$RemoteUser = 'root',
    [switch]$OpenFirewall
)

$ErrorActionPreference = 'Stop'
$project = Split-Path $PSScriptRoot -Parent
$release = (Resolve-Path -LiteralPath $ReleaseDirectory).Path
$identity = (Resolve-Path -LiteralPath $IdentityFile).Path
$manifestPath = Join-Path $release 'latest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 | ConvertFrom-Json
$version = [string]$manifest.version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'Release manifest has an invalid version.' }

$referralCatalogPath = $null
if ($ReferralCatalogDirectory) {
    $referralDirectory = (Resolve-Path -LiteralPath $ReferralCatalogDirectory).Path
    $referralCatalogPath = Join-Path $referralDirectory 'latest.json'
    $referralEnvelope = Get-Content -LiteralPath $referralCatalogPath -Raw -Encoding utf8 | ConvertFrom-Json
    if ([int]$referralEnvelope.schemaVersion -ne 1 -or
        [string]::IsNullOrWhiteSpace([string]$referralEnvelope.payload) -or
        [string]::IsNullOrWhiteSpace([string]$referralEnvelope.signature)) {
        throw 'Referral catalog envelope is incomplete or has an unsupported schema.'
    }
}

$setupName = "AI-Route-Manager-$version-Windows-x64-setup.exe"
$portableName = "AI-Route-Manager-$version-Windows-x64-portable.zip"
$signatureName = "$setupName.sig"
foreach ($name in @($setupName, $portableName, $signatureName, 'SHA256SUMS.txt')) {
    if (-not (Test-Path -LiteralPath (Join-Path $release $name) -PathType Leaf)) {
        throw "Release file is missing: $name"
    }
}

$platform = $manifest.platforms.'windows-x86_64'
$artifactUri = [Uri]$platform.url
if ($artifactUri.Scheme -ne 'https' -or $artifactUri.Host -ne $Domain) {
    throw "Manifest artifact URL must use https://$Domain/."
}
$signature = (Get-Content -LiteralPath (Join-Path $release $signatureName) -Raw -Encoding utf8).Trim()
if ($platform.signature -cne $signature) { throw 'Manifest signature does not match the signature file.' }

$hashes = @{}
foreach ($line in Get-Content -LiteralPath (Join-Path $release 'SHA256SUMS.txt')) {
    if ($line -notmatch '^([0-9A-Fa-f]{64})  (.+)$') { throw "Invalid SHA256SUMS row: $line" }
    $hashes[$matches[2]] = $matches[1].ToUpperInvariant()
}
foreach ($name in @($setupName, $portableName, $signatureName, 'latest.json')) {
    $actual = (Get-FileHash -LiteralPath (Join-Path $release $name) -Algorithm SHA256).Hash
    if ($hashes[$name] -ne $actual) { throw "SHA-256 mismatch: $name" }
}

$stage = [IO.Path]::GetFullPath((Join-Path $env:TEMP ("ai-route-update-" + [guid]::NewGuid().ToString('N'))))
$temporaryRoot = [IO.Path]::GetFullPath($env:TEMP).TrimEnd('\') + '\'
if (-not $stage.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Generated staging path escaped the Windows temporary directory.'
}
New-Item -ItemType Directory -Path (Join-Path $stage "site\ai-route-manager\releases\$version") -Force | Out-Null
if ($referralCatalogPath) {
    New-Item -ItemType Directory -Path (Join-Path $stage 'site\ai-route-manager\referrals') -Force | Out-Null
}
New-Item -ItemType Directory -Path (Join-Path $stage 'data') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $stage 'config') -Force | Out-Null
try {
    $remoteRelease = Join-Path $stage "site\ai-route-manager\releases\$version"
    foreach ($name in @($setupName, $portableName, $signatureName, 'SHA256SUMS.txt', 'latest.json')) {
        Copy-Item -LiteralPath (Join-Path $release $name) -Destination $remoteRelease -Force
    }
    Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $stage 'site\ai-route-manager\latest.json') -Force
    if ($referralCatalogPath) {
        Copy-Item -LiteralPath $referralCatalogPath -Destination (Join-Path $stage 'site\ai-route-manager\referrals\latest.json') -Force
    }
    Copy-Item -LiteralPath (Join-Path $project 'infra\update-feed\docker-compose.yml') -Destination (Join-Path $stage 'docker-compose.yml') -Force
    $caddy = (Get-Content -LiteralPath (Join-Path $project 'infra\update-feed\Caddyfile.template') -Raw -Encoding utf8).Replace('__DOMAIN__', $Domain)
    Set-Content -LiteralPath (Join-Path $stage 'Caddyfile') -Value $caddy -Encoding utf8
    $base = "https://$Domain/ai-route-manager/releases/$version"
    $index = (Get-Content -LiteralPath (Join-Path $project 'infra\update-feed\index.html.template') -Raw -Encoding utf8).
        Replace('__VERSION__', $version).
        Replace('__SETUP__', "$base/$setupName").
        Replace('__PORTABLE__', "$base/$portableName").
        Replace('__MANIFEST__', "https://$Domain/ai-route-manager/latest.json").
        Replace('__SETUP_SHA__', $hashes[$setupName]).
        Replace('__PORTABLE_SHA__', $hashes[$portableName])
    Set-Content -LiteralPath (Join-Path $stage 'site\index.html') -Value $index -Encoding utf8

    $remote = "$RemoteUser@$RemoteHost"
    $incoming = "/opt/ai-route-updates.incoming-$version-$([guid]::NewGuid().ToString('N'))"
    & scp.exe -i $identity -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -r $stage "${remote}:$incoming"
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $remoteScript = @'
set -eu
incoming="$1"
target=/opt/ai-route-updates
test -f "$incoming/Caddyfile"
test -f "$incoming/docker-compose.yml"
docker run --rm -v "$incoming/Caddyfile:/etc/caddy/Caddyfile:ro" caddy:2.11.4-alpine@sha256:6aeddd44c3078b0f9a35206472a11420648a79c184603ef95957d0a20044cb2b caddy validate --config /etc/caddy/Caddyfile
mkdir -p "$target/site/ai-route-manager/releases" "$target/data" "$target/config"
cp "$incoming/Caddyfile" "$target/Caddyfile"
cp "$incoming/docker-compose.yml" "$target/docker-compose.yml"
cp "$incoming/site/index.html" "$target/site/index.html"
cp -a "$incoming/site/ai-route-manager/releases/." "$target/site/ai-route-manager/releases/"
cp "$incoming/site/ai-route-manager/latest.json" "$target/site/ai-route-manager/latest.json.new"
mv "$target/site/ai-route-manager/latest.json.new" "$target/site/ai-route-manager/latest.json"
if [ -f "$incoming/site/ai-route-manager/referrals/latest.json" ]; then
    mkdir -p "$target/site/ai-route-manager/referrals"
    cp "$incoming/site/ai-route-manager/referrals/latest.json" "$target/site/ai-route-manager/referrals/latest.json.new"
    mv "$target/site/ai-route-manager/referrals/latest.json.new" "$target/site/ai-route-manager/referrals/latest.json"
fi
cd "$target"
if [ "$2" = "True" ]; then
    ufw allow 80/tcp comment 'AI Route Manager HTTPS redirect'
    ufw allow 443/tcp comment 'AI Route Manager HTTPS updates'
    ufw allow 443/udp comment 'AI Route Manager HTTP3 updates'
fi
docker compose up -d
docker compose exec -T caddy caddy reload --config /etc/caddy/Caddyfile </dev/null
'@
    $remoteScriptBytes = [Text.Encoding]::UTF8.GetBytes($remoteScript.Replace("`r", ""))
    $remoteScriptBase64 = [Convert]::ToBase64String($remoteScriptBytes)
    $firewallFlag = [string][bool]$OpenFirewall
    $remoteCommand = "printf '%s' '$remoteScriptBase64' | base64 -d | sh -s -- '$incoming' '$firewallFlag'"
    & ssh.exe -i $identity -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes $remote $remoteCommand
    $remoteExit = $LASTEXITCODE
    & ssh.exe -i $identity -o BatchMode=yes -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes $remote rm -rf -- $incoming
    $cleanupExit = $LASTEXITCODE
    if ($remoteExit -ne 0) { exit $remoteExit }
    if ($cleanupExit -ne 0) { exit $cleanupExit }
    Write-Host "UPDATE_FEED_DEPLOYED=https://$Domain/ai-route-manager/latest.json"
    Write-Host "UPDATE_FEED_VERSION=$version"
    if ($referralCatalogPath) {
        Write-Host "REFERRAL_CATALOG_DEPLOYED=https://$Domain/ai-route-manager/referrals/latest.json"
    }
}
finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
}
