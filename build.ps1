$ErrorActionPreference = "Stop"

function Rename-And-MoveBundle {
    param(
        [string]$BundlePath,
        [string]$Suffix,
        [switch]$Debug
    )

    if (-not (Test-Path $BundlePath)) {
        return
    }

    Get-ChildItem -Path $BundlePath -File | ForEach-Object {
        $newName = $_.Name -replace 'ShadowVerse', "ShadowVerse-$Suffix"
        $newName = $newName -replace 'shadowverse', "shadowverse-$Suffix"

        if ($Debug) {
            $newName = $newName -replace '\.msi$', '-debug.msi'
            $newName = $newName -replace '\.exe$', '-debug.exe'
        }

        Rename-Item -Path $_.FullName -NewName $newName
        Move-Item -Path (Join-Path $_.DirectoryName $newName) -Destination ./src-tauri/target/
    }
}

function Test-CudaResources {
    $patterns = @("cudart64*.dll", "cublas64*.dll", "cublasLt64*.dll")
    foreach ($pattern in $patterns) {
        $found = Get-ChildItem -Path ./src-tauri -Filter $pattern -ErrorAction SilentlyContinue
        if (-not $found) {
            return $false
        }
    }
    return $true
}

# CPU build (default config)
yarn tauri build
yarn tauri build --debug

Rename-And-MoveBundle -BundlePath ./src-tauri/target/release/bundle/msi -Suffix "cpu"
Rename-And-MoveBundle -BundlePath ./src-tauri/target/release/bundle/nsis -Suffix "cpu"
Rename-And-MoveBundle -BundlePath ./src-tauri/target/debug/bundle/msi -Suffix "cpu" -Debug
Rename-And-MoveBundle -BundlePath ./src-tauri/target/debug/bundle/nsis -Suffix "cpu" -Debug

# CUDA build (override config)
if (Test-CudaResources) {
    yarn tauri build --config src-tauri/tauri.windows.cuda.conf.json
    yarn tauri build --debug --config src-tauri/tauri.windows.cuda.conf.json
} else {
    Write-Host "Skip CUDA build: missing cudart/cublas DLLs in ./src-tauri" -ForegroundColor Yellow
}

Rename-And-MoveBundle -BundlePath ./src-tauri/target/release/bundle/msi -Suffix "cuda"
Rename-And-MoveBundle -BundlePath ./src-tauri/target/release/bundle/nsis -Suffix "cuda"
Rename-And-MoveBundle -BundlePath ./src-tauri/target/debug/bundle/msi -Suffix "cuda" -Debug
Rename-And-MoveBundle -BundlePath ./src-tauri/target/debug/bundle/nsis -Suffix "cuda" -Debug
