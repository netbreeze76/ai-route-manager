$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
$engineBuild = Join-Path $project 'work\engine-build'
$dist = Join-Path $engineBuild 'dist'
$build = Join-Path $engineBuild 'build'
$source = Join-Path $project 'engine\dynamic_proxy_manager.py'
$tests = Join-Path $project 'engine'
$staging = Join-Path $project 'src-tauri\resources\engine'
$pythonCommand = Get-Command python -ErrorAction SilentlyContinue
if (-not $pythonCommand) { $pythonCommand = Get-Command py -ErrorAction SilentlyContinue }
if (-not $pythonCommand) { throw 'Python 3 is required to build DynamicProxyEngine.' }
$python = $pythonCommand.Source

foreach ($required in @($source, (Join-Path $staging 'bin\mihomo.exe'))) {
    if (-not (Test-Path -LiteralPath $required)) {
        throw "Required engine build input is missing: $required"
    }
}

Push-Location $engineBuild
try {
    & $python -m pip install --disable-pip-version-check --requirement (Join-Path $PSScriptRoot 'requirements-engine.txt')
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & $python -m PyInstaller --noconfirm --clean --onefile --name DynamicProxyEngine --distpath $dist --workpath $build --specpath $engineBuild $source
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}
finally {
    Pop-Location
}

$builtEngine = Join-Path $dist 'DynamicProxyEngine.exe'
if (-not (Test-Path -LiteralPath $builtEngine)) {
    throw "PyInstaller did not create $builtEngine"
}

Copy-Item -LiteralPath $builtEngine -Destination (Join-Path $staging 'DynamicProxyEngine.exe') -Force

$manifest = Join-Path $staging 'MANIFEST.sha256'
$lines = foreach ($file in Get-ChildItem -LiteralPath $staging -File -Recurse | Where-Object Name -ne 'MANIFEST.sha256' | Sort-Object FullName) {
    $relative = $file.FullName.Substring($staging.Length + 1).Replace('\', '/')
    $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
    "$hash  $relative"
}
Set-Content -LiteralPath $manifest -Value $lines -Encoding utf8

& $python -m unittest discover -s $tests -p 'test_*.py' -v
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$builtHash = (Get-FileHash -LiteralPath $builtEngine -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath (Join-Path $staging 'DynamicProxyEngine.exe') -Algorithm SHA256).Hash
if ($builtHash -ne $stagedHash) {
    throw 'Staged engine hash does not match the reproducible build output.'
}

Write-Host "ENGINE_BUILD_OK SHA256=$builtHash"
Write-Host "ENGINE_STAGED=$staging"
