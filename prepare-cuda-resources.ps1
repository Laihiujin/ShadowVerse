$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$cudaPath = $env:CUDA_PATH
if (-not $cudaPath) {
    throw "CUDA_PATH is not set."
}

$cudaBinPath = Join-Path $cudaPath "bin"
if (-not (Test-Path $cudaBinPath)) {
    throw "CUDA bin directory not found: $cudaBinPath"
}

$destinationPath = Join-Path $PSScriptRoot "src-tauri"
if (-not (Test-Path $destinationPath)) {
    throw "Destination directory not found: $destinationPath"
}

$patterns = @("cudart64*.dll", "cublas64*.dll", "cublasLt64*.dll")
$copied = [System.Collections.Generic.List[string]]::new()

foreach ($pattern in $patterns) {
    $files = Get-ChildItem -Path $cudaBinPath -Filter $pattern -File -ErrorAction SilentlyContinue
    if (-not $files) {
        throw "Missing CUDA runtime matching '$pattern' in $cudaBinPath"
    }

    foreach ($file in $files) {
        Copy-Item -LiteralPath $file.FullName -Destination $destinationPath -Force
        $copied.Add($file.Name)
    }
}

$copied |
    Sort-Object -Unique |
    ForEach-Object { Write-Host "Prepared CUDA resource: $_" }
