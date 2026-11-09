$ErrorActionPreference = 'Stop'

$project = Split-Path $PSScriptRoot -Parent
$workspace = Split-Path $project -Parent
$rustBin = Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-gnu\lib\rustlib\x86_64-pc-windows-gnu\bin'
$llvmBin = Join-Path $workspace 'work\llvm-mingw\toolchain-msvcrt-20261020\llvm-mingw-20261020-msvcrt-x86_64\bin'

if (-not (Test-Path (Join-Path $rustBin 'rust-lld.exe'))) {
    throw 'GNU Rust toolchain missing. Run: rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal'
}
if (-not (Test-Path (Join-Path $llvmBin 'x86_64-w64-mingw32-gcc.exe'))) {
    throw "LLVM-MinGW toolchain missing: $llvmBin"
}

$env:PATH = "$env:USERPROFILE\.cargo\bin;$rustBin;$llvmBin;$env:PATH"
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-gnu'
$env:RUSTFLAGS = '-C linker=rust-lld.exe -C link-self-contained=yes'
$env:CC = 'x86_64-w64-mingw32-gcc.exe'
$env:CXX = 'x86_64-w64-mingw32-g++.exe'
$env:AR = 'llvm-ar.exe'

& cargo test --manifest-path (Join-Path $project 'src-tauri\Cargo.toml') --lib --no-default-features
exit $LASTEXITCODE
