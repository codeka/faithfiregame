param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot

try {
    if ($Release) {
        Write-Host "Building client-wasm (RELEASE, no editor)..."
        cargo build -p client-wasm --target wasm32-unknown-unknown --release
        $wasmPath = Join-Path $RepoRoot "target\wasm32-unknown-unknown\release\client_wasm.wasm"
    } else {
        Write-Host "Building client-wasm (DEV + editor)..."
        cargo build -p client-wasm --target wasm32-unknown-unknown --features editor
        $wasmPath = Join-Path $RepoRoot "target\wasm32-unknown-unknown\debug\client_wasm.wasm"
    }

    if ($LASTEXITCODE -ne 0) {
        throw "cargo build failed with exit code $LASTEXITCODE"
    }

    $jsOutDir = Join-Path $RepoRoot "web\src\app\wasm"
    $publicWasmDir = Join-Path $RepoRoot "web\public\wasm"
    New-Item -ItemType Directory -Force -Path $jsOutDir | Out-Null
    New-Item -ItemType Directory -Force -Path $publicWasmDir | Out-Null

    Write-Host "Running wasm-bindgen..."
    wasm-bindgen --target web --out-dir $jsOutDir $wasmPath
    if ($LASTEXITCODE -ne 0) {
        throw "wasm-bindgen failed with exit code $LASTEXITCODE"
    }

    Copy-Item (Join-Path $jsOutDir "client_wasm_bg.wasm") (Join-Path $publicWasmDir "client_wasm_bg.wasm") -Force
    Write-Host "WASM build ready -> $publicWasmDir\client_wasm_bg.wasm"
}
finally {
    Pop-Location
}
