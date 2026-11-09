$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
$rustBin = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin'

$llvmBin = $env:LLVM_MINGW_BIN
if (-not $llvmBin) {
    $cursor = Get-Item -LiteralPath $project
    while ($cursor) {
        $candidateRoot = Join-Path $cursor.FullName 'work\llvm-mingw'
        if (Test-Path -LiteralPath $candidateRoot) {
            $compiler = Get-ChildItem -LiteralPath $candidateRoot -Filter 'x86_64-w64-mingw32-gcc.exe' -File -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($compiler) {
                $llvmBin = $compiler.DirectoryName
                break
            }
        }
        $cursor = $cursor.Parent
    }
}

if (-not (Test-Path (Join-Path $rustBin 'rust-lld.exe'))) {
    throw 'GNU Rust toolchain missing. Run: rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal'
}
if (-not $llvmBin -or -not (Test-Path (Join-Path $llvmBin 'x86_64-w64-mingw32-gcc.exe'))) {
    throw 'LLVM-MinGW toolchain missing. Set LLVM_MINGW_BIN to its bin directory.'
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$rustBin;$llvmBin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:RUSTFLAGS = '-C linker=rust-lld.exe -C link-self-contained=yes'
$env:CC = 'x86_64-w64-mingw32-gcc.exe'
$env:CXX = 'x86_64-w64-mingw32-g++.exe'
$env:AR = 'llvm-ar.exe'

& cargo test --manifest-path (Join-Path $project 'src-tauri\Cargo.toml') --lib --no-default-features
exit $LASTEXITCODE
