# Build the branded Codex CLI for Windows (and optionally macOS/Linux).
# Requires: rustup + MSVC toolchain, Python 3, and network access for crates/V8.
# Usage (PowerShell):
#   .\scripts\build_cnapi_packages.ps1 -NewApiBaseUrl "https://newapi.example.com" -Targets x86_64-pc-windows-msvc,aarch64-apple-darwin
param(
    [string]$NewApiBaseUrl = "https://new.ahei.asia",
    [string]$ProductName = "AHEIAPI",
    [string[]]$Targets = @("x86_64-pc-windows-msvc"),
    [string]$Profile = "release",
    [string]$Variant = "codex",
    [string]$DistDir = ""
)

$ErrorActionPreference = "Stop"

# Resolve repo root: parent of the scripts directory.
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $DistDir) {
    $DistDir = Join-Path $RepoRoot "dist"
}
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

# The packager requires CODEX_REPO_ROOT to locate the repo and codex-rs.
$env:CODEX_REPO_ROOT = $RepoRoot

# Optional product display name baked into the binary.
if ($ProductName) {
    $env:NEWAPI_PRODUCT_NAME = $ProductName
    Write-Host "Product name: $ProductName"
} else {
    Remove-Item Env:\NEWAPI_PRODUCT_NAME -ErrorAction SilentlyContinue
}

# Optional build-time default relay URL baked into the binary.
if ($NewApiBaseUrl) {
    $env:NEWAPI_BASE_URL = $NewApiBaseUrl
    Write-Host "Baking in default New API base URL: $NewApiBaseUrl"
} else {
    Remove-Item Env:\NEWAPI_BASE_URL -ErrorAction SilentlyContinue
}

foreach ($cmd in @("cargo", "rustc", "python")) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        throw "Missing required tool: $cmd. Install Rust toolchain and Python first."
    }
}

$script = Join-Path $RepoRoot "scripts\build_codex_package.py"
if (-not (Test-Path $script)) {
    throw "Packager not found: $script"
}

foreach ($target in $Targets) {
    $target = $target.Trim()
    if ($target -eq "x86_64-pc-windows-msvc" -or $target -eq "aarch64-pc-windows-msvc") {
        $ext = "zip"
    } else {
        $ext = "tar.gz"
    }
    $safe = $target -replace "[^A-Za-z0-9_.-]", "_"
    $archive = Join-Path $DistDir ("{0}-{1}.{2}" -f $Variant, $safe, $ext)
    Write-Host "`n=== Building $target -> $archive ==="
    python $script `
        --variant $Variant `
        --target $target `
        --cargo-profile $Profile `
        --archive-output $archive `
        --force
    if (-not (Test-Path $archive)) {
        throw "Expected archive not produced: $archive"
    }
    Write-Host "OK: $archive"
}

Write-Host "`nAll packages built. Outputs:"
Get-ChildItem $DistDir -File | Select-Object Name, Length
