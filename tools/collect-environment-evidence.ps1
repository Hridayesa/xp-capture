[CmdletBinding()]
param(
    [string]$OutputPath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $OutputPath) {
    $OutputPath = Join-Path $repositoryRoot "evidence\environment-summary.json"
}

& pwsh -NoProfile -File (Join-Path $PSScriptRoot "verify-environment.ps1") -EvidencePath (Join-Path $repositoryRoot "evidence\environment.json")
if ($LASTEXITCODE -ne 0) {
    throw "Environment prerequisite verification failed"
}

$environment = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "evidence\environment.json") | ConvertFrom-Json -Depth 20
$native = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "evidence\native-environment.json") | ConvertFrom-Json -Depth 20
$runtimeManifestPath = Join-Path $repositoryRoot "runtime\manifest.json"
$runtimeManifest = Get-Content -Raw -LiteralPath $runtimeManifestPath | ConvertFrom-Json -Depth 20
$package = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "package.json") | ConvertFrom-Json -Depth 20
$cargoManifest = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "src-tauri\Cargo.toml")
$cargoLock = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "Cargo.lock")
$rustToolchain = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "rust-toolchain.toml")

function Get-Check {
    param([Parameter(Mandatory)][string]$Name)

    $check = $environment.checks | Where-Object name -EQ $Name | Select-Object -First 1
    if (-not $check -or $check.status -ne "passed") {
        throw "Required environment check is not passed: $Name"
    }
    $check
}

function Get-CargoDependencyVersion {
    param([Parameter(Mandatory)][string]$Name)

    $pattern = '(?m)^{0}\s*=\s*\{{\s*version\s*=\s*"=(?<version>[^\"]+)"' -f [regex]::Escape($Name)
    $match = [regex]::Match($cargoManifest, $pattern)
    if (-not $match.Success) {
        throw "Pinned Cargo dependency was not found: $Name"
    }
    $match.Groups["version"].Value
}

function Get-CargoLockVersion {
    param([Parameter(Mandatory)][string]$Name)

    $pattern = '(?ms)^\[\[package\]\]\s*name = "{0}"\s*version = "(?<version>[^\"]+)"' -f [regex]::Escape($Name)
    $match = [regex]::Match($cargoLock, $pattern)
    if (-not $match.Success) {
        throw "Cargo.lock package was not found: $Name"
    }
    $match.Groups["version"].Value
}

$msvc = Get-Check -Name "msvc_x64"
$sdk = Get-Check -Name "windows_sdk"
$rust = Get-Check -Name "rust"
$bun = Get-Check -Name "bun"
$cmake = Get-Check -Name "cmake"
$ninja = Get-Check -Name "ninja"
$powershell = Get-Check -Name "powershell"
$vcpkgRevision = (& git -C (Join-Path $repositoryRoot ".tools\vcpkg") rev-parse "HEAD^{commit}").Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Cannot read repository-local vcpkg revision"
}
$repositoryCommit = (& git -C $repositoryRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Cannot read repository commit"
}
$msvcEntry = $runtimeManifest.files | Where-Object source -Like ".tools/msvc-redist/*" | Select-Object -First 1
$msvcVersionMatch = [regex]::Match($msvcEntry.source, '^\.tools/msvc-redist/(?<version>[^/]+)/')
if (-not $msvcVersionMatch.Success) {
    throw "Runtime manifest does not expose an MSVC Redistributable version"
}

$evidence = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    repositoryCommit = $repositoryCommit
    host = [ordered]@{
        windowsVersion = [Environment]::OSVersion.Version.ToString()
        architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
        powershell = $powershell.version
        msvc = $msvc.version
        windowsSdk = $sdk.version
        cmake = (($cmake.version -split "\r?\n")[0] -replace '^cmake version\s+', '')
        ninja = $ninja.version
    }
    application = [ordered]@{
        rust = ([regex]::Match($rustToolchain, 'channel\s*=\s*"(?<version>[^"]+)"').Groups["version"].Value)
        bun = ($package.packageManager -replace '^bun@', '')
        tauriCrate = Get-CargoDependencyVersion -Name "tauri"
        tauriBuild = Get-CargoDependencyVersion -Name "tauri-build"
        tauriCli = ($package.devDependencies.'@tauri-apps/cli')
        tauriApi = ($package.dependencies.'@tauri-apps/api')
        opencvCrate = Get-CargoLockVersion -Name "opencv"
    }
    nativeSupply = [ordered]@{
        vcpkgRevision = $vcpkgRevision
        triplet = $native.triplet
        opencvVersion = $native.package.version
        opencvPortVersion = $native.package.portVersion
        opencvFeatures = @($native.package.features)
        msvcRedistributable = $msvcVersionMatch.Groups["version"].Value
    }
    runtimeManifest = [ordered]@{
        sha256 = (Get-FileHash -LiteralPath $runtimeManifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
        fileCount = @($runtimeManifest.files).Count
    }
}

$resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputPath, $repositoryRoot)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $resolvedOutputPath) | Out-Null
$evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $resolvedOutputPath -Encoding utf8NoBOM

& bun (Join-Path $PSScriptRoot "validate-environment-evidence.mjs") $resolvedOutputPath
if ($LASTEXITCODE -ne 0) {
    throw "Environment summary failed consistency validation"
}
Write-Output "Environment evidence collected: $resolvedOutputPath"
