[CmdletBinding()]
param(
    [string]$VcpkgRoot = "",
    [string]$InstallRoot = "",
    [string]$EvidencePath = "",
    [string]$EnvironmentProbeFixturePath = "",
    [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$PinnedRevision = "9e593bb18ea69cc5095e012465dcd675a822ed0d"
$Triplet = "x64-windows"
$OpenCvVersion = "4.12.0"
$OpenCvPortVersion = "7"
$RequiredFeatures = @("dshow", "ffmpeg", "jpeg", "msmf", "thread")
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Invoke-Checked {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [Parameter(ValueFromRemainingArguments)][string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

function Get-PeeledRevision {
    param([Parameter(Mandatory)][string]$RepositoryPath)

    $revision = (& git -C $RepositoryPath rev-parse "HEAD^{commit}" 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $revision) {
        throw "Cannot determine vcpkg revision in $RepositoryPath"
    }
    $revision
}

function Initialize-PinnedVcpkg {
    param([Parameter(Mandatory)][string]$Destination)

    if (Test-Path -LiteralPath $Destination) {
        if (-not (Test-Path -LiteralPath (Join-Path $Destination ".git") -PathType Container)) {
            throw "Vcpkg root exists but is not a Git checkout: $Destination"
        }

        $actualRevision = Get-PeeledRevision -RepositoryPath $Destination
        if ($actualRevision -ne $PinnedRevision) {
            throw "Vcpkg revision mismatch. Expected $PinnedRevision, actual $actualRevision"
        }
        return $actualRevision
    }

    $parent = Split-Path -Parent $Destination
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $staging = Join-Path $parent "vcpkg-bootstrap"
    if (Test-Path -LiteralPath $staging) {
        throw "Incomplete vcpkg staging directory exists: $staging. Remove this exact directory before retrying."
    }

    New-Item -ItemType Directory -Path $staging | Out-Null
    try {
        Invoke-Checked git -C $staging init
        Invoke-Checked git -C $staging remote add origin https://github.com/microsoft/vcpkg.git
        Invoke-Checked git -C $staging -c protocol.version=2 fetch --depth 1 origin $PinnedRevision
        Invoke-Checked git -C $staging checkout --detach FETCH_HEAD

        $actualRevision = Get-PeeledRevision -RepositoryPath $staging
        if ($actualRevision -ne $PinnedRevision) {
            throw "Fetched vcpkg revision mismatch. Expected $PinnedRevision, actual $actualRevision"
        }

        Move-Item -LiteralPath $staging -Destination $Destination
        return $actualRevision
    } catch {
        throw "Failed to prepare pinned vcpkg checkout in $staging. $($_.Exception.Message)"
    }
}

function Get-InstalledOpenCvMetadata {
    param([Parameter(Mandatory)][string]$Root)

    $statusPath = Join-Path $Root "vcpkg\status"
    if (-not (Test-Path -LiteralPath $statusPath -PathType Leaf)) {
        throw "Vcpkg status database is missing: $statusPath"
    }

    $paragraphs = (Get-Content -Raw -LiteralPath $statusPath) -split "(?:\r?\n){2,}"
    $opencvParagraphs = @($paragraphs | Where-Object {
        $_ -match "(?m)^Package: opencv4$" -and $_ -match "(?m)^Architecture: $([regex]::Escape($Triplet))$"
    })
    $base = $opencvParagraphs | Where-Object { $_ -notmatch "(?m)^Feature:" } | Select-Object -First 1
    if (-not $base) {
        throw "opencv4:$Triplet is not installed"
    }

    $version = [regex]::Match($base, "(?m)^Version: (.+)$").Groups[1].Value.Trim()
    $portVersionMatch = [regex]::Match($base, "(?m)^Port-Version: (.+)$")
    $portVersion = if ($portVersionMatch.Success) { $portVersionMatch.Groups[1].Value.Trim() } else { "0" }
    $features = @($opencvParagraphs | ForEach-Object {
        $match = [regex]::Match($_, "(?m)^Feature: (.+)$")
        if ($match.Success) { $match.Groups[1].Value.Trim() }
    } | Sort-Object -Unique)

    if ($version -ne $OpenCvVersion -or $portVersion -ne $OpenCvPortVersion) {
        throw "opencv4 port mismatch. Expected $OpenCvVersion#$OpenCvPortVersion, actual $version#$portVersion"
    }
    foreach ($feature in $RequiredFeatures) {
        if ($feature -notin $features) {
            throw "opencv4 feature is missing for ${Triplet}: $feature"
        }
    }

    [ordered]@{
        name = "opencv4"
        version = $version
        portVersion = [int]$portVersion
        triplet = $Triplet
        defaultFeatures = $false
        features = $RequiredFeatures
    }
}

if (-not $VcpkgRoot) {
    $VcpkgRoot = Join-Path $repositoryRoot ".tools\vcpkg"
}
if (-not $InstallRoot) {
    $InstallRoot = Join-Path $repositoryRoot ".vcpkg_installed"
}
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repositoryRoot "evidence\native-environment.json"
}

$environmentArgs = @("-NoProfile", "-File", (Join-Path $PSScriptRoot "verify-environment.ps1"), "-EvidencePath", (Join-Path $repositoryRoot "evidence\environment.json"))
if ($EnvironmentProbeFixturePath) {
    $environmentArgs += @("-ProbeFixturePath", $EnvironmentProbeFixturePath)
}
Invoke-Checked pwsh @environmentArgs

$actualRevision = Initialize-PinnedVcpkg -Destination ([System.IO.Path]::GetFullPath($VcpkgRoot, $repositoryRoot))
if ($SkipInstall) {
    Write-Output "Pinned vcpkg revision verified; install skipped by explicit test switch."
    exit 0
}

$resolvedVcpkgRoot = [System.IO.Path]::GetFullPath($VcpkgRoot, $repositoryRoot)
$resolvedInstallRoot = [System.IO.Path]::GetFullPath($InstallRoot, $repositoryRoot)
$vcpkgExecutable = Join-Path $resolvedVcpkgRoot "vcpkg.exe"
if (-not (Test-Path -LiteralPath $vcpkgExecutable -PathType Leaf)) {
    Invoke-Checked (Join-Path $resolvedVcpkgRoot "bootstrap-vcpkg.bat") -disableMetrics
}

$previousVcpkgRoot = $env:VCPKG_ROOT
$previousTriplet = $env:VCPKG_DEFAULT_TRIPLET
$previousFeatureFlags = $env:VCPKG_FEATURE_FLAGS
try {
    $env:VCPKG_ROOT = $resolvedVcpkgRoot
    $env:VCPKG_DEFAULT_TRIPLET = $Triplet
    $env:VCPKG_FEATURE_FLAGS = "manifests,versions"
    Invoke-Checked $vcpkgExecutable install "--triplet=$Triplet" "--x-manifest-root=$repositoryRoot" "--x-install-root=$resolvedInstallRoot" --disable-metrics
} finally {
    $env:VCPKG_ROOT = $previousVcpkgRoot
    $env:VCPKG_DEFAULT_TRIPLET = $previousTriplet
    $env:VCPKG_FEATURE_FLAGS = $previousFeatureFlags
}

$opencv = Get-InstalledOpenCvMetadata -Root $resolvedInstallRoot
$evidence = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    vcpkgRevision = $actualRevision
    vcpkgRoot = $resolvedVcpkgRoot
    installRoot = $resolvedInstallRoot
    triplet = $Triplet
    package = $opencv
}
$resolvedEvidencePath = [System.IO.Path]::GetFullPath($EvidencePath, $repositoryRoot)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $resolvedEvidencePath) | Out-Null
$evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $resolvedEvidencePath -Encoding utf8NoBOM

$generatorArguments = @(
    "-NoProfile",
    "-File",
    (Join-Path $PSScriptRoot "generate-runtime-manifest.ps1"),
    "-InstallRoot",
    $resolvedInstallRoot
)
& pwsh @generatorArguments
if ($LASTEXITCODE -ne 0) {
    throw "Runtime manifest generation failed with exit code ${LASTEXITCODE}"
}

Write-Output "OpenCV native environment is ready. Evidence: $resolvedEvidencePath"
