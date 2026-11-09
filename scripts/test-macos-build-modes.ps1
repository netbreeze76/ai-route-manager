$ErrorActionPreference = 'Stop'
$buildPath = Join-Path $PSScriptRoot 'build-macos.sh'
$verifyPath = Join-Path $PSScriptRoot 'verify-macos-bundle.sh'
$build = Get-Content -LiteralPath $buildPath -Raw -Encoding utf8
$verify = Get-Content -LiteralPath $verifyPath -Raw -Encoding utf8

$checks = [ordered]@{
    DefaultModeIsLocalTest = $build.Contains('MODE="${1:---local-test}"')
    LocalTestDisablesUpdaterArtifacts = $build.Contains('{"bundle":{"createUpdaterArtifacts":false}}')
    LocalTestClearsUpdaterSecrets = $build.Contains('unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD')
    LocalTestUsesAdHocIdentity = $build.Contains('export APPLE_SIGNING_IDENTITY="-"')
    ReleaseRequiresUpdaterKey = $build.Contains('Release builds require the existing updater private key')
    ReleaseRejectsAdHoc = $build.Contains('release mode rejects ad-hoc signing')
    ReleaseRequiresUpdaterSignature = $build.Contains('release mode did not create updater signature')
    LocalAndReleaseOutputsAreSeparated = $build.Contains('RELEASE_KIND="macos-local-test"') -and $build.Contains('RELEASE_KIND="macos"')
    GatekeeperTreatsDashAsAdHoc = $verify.Contains('"$APPLE_SIGNING_IDENTITY" != "-"')
    UnnotarizedReleaseEscapeRemoved = -not $build.Contains('AI_ROUTE_ALLOW_UNNOTARIZED')
}

$failed = @($checks.GetEnumerator() | Where-Object { -not $_.Value })
if ($failed.Count -gt 0) {
    $failed | ForEach-Object { Write-Error "MACOS_BUILD_MODE_CHECK_FAILED=$($_.Key)" }
    exit 1
}

Write-Output "MACOS_BUILD_MODE_TESTS_OK=$($checks.Count)"
exit 0
