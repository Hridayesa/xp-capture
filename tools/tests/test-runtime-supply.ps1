[CmdletBinding()]
param(
    [string]$ExecutablePath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$manifestPath = Join-Path $repositoryRoot "runtime\manifest.json"
$stagingRoot = Join-Path $repositoryRoot "runtime\staging"
if (-not $ExecutablePath) {
    $ExecutablePath = Join-Path $repositoryRoot "target\debug\xp-capture.exe"
}
$resolvedExecutable = [System.IO.Path]::GetFullPath($ExecutablePath, $repositoryRoot)
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "Runtime supply test requires a built x64 executable: $resolvedExecutable"
}

function Invoke-CheckedPowerShell {
    param([Parameter(Mandatory)][string]$ScriptPath, [string[]]$Arguments = @())

    & pwsh -NoProfile -File $ScriptPath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "PowerShell command failed with exit code ${LASTEXITCODE}: $ScriptPath"
    }
}

function Invoke-ExpectedFailure {
    param(
        [Parameter(Mandatory)][string]$ScriptPath,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$ExpectedMessage
    )

    $output = (& pwsh -NoProfile -File $ScriptPath @Arguments 2>&1 | Out-String)
    if ($LASTEXITCODE -eq 0) {
        throw "Expected command failure, but it succeeded: $ScriptPath"
    }
    if ($output -notmatch [regex]::Escape($ExpectedMessage)) {
        throw "Expected failure message '$ExpectedMessage' was not found. Output: $output"
    }
}

$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("xp-capture-runtime-supply-" + [Guid]::NewGuid().ToString("N"))
$hashMismatchStaging = Join-Path $repositoryRoot "runtime\test-staging-hash-mismatch"
New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
try {
    $manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json -Depth 20
    $requiredVcRuntime = @("CONCRT140.dll", "MSVCP140.dll", "VCRUNTIME140.dll", "VCRUNTIME140_1.dll")
    foreach ($name in $requiredVcRuntime) {
        $entry = @($manifest.files | Where-Object name -EQ $name)
        if ($entry.Count -ne 1 -or $entry[0].purpose -ne "transitive" -or $entry[0].source -notlike ".tools/msvc-redist/*") {
            throw "Runtime manifest does not contain exactly one app-local MSVC Redistributable entry for $name"
        }
        $source = [System.IO.Path]::GetFullPath(($entry[0].source -replace '/', '\'), $repositoryRoot)
        $license = [System.IO.Path]::GetFullPath(($entry[0].licenseNoticePath -replace '/', '\'), $repositoryRoot)
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or -not (Test-Path -LiteralPath $license -PathType Leaf)) {
            throw "MSVC Redistributable source or notice is missing for $name"
        }
    }

    $stageScript = Join-Path $repositoryRoot "tools\stage-runtime.ps1"
    Invoke-CheckedPowerShell -ScriptPath $stageScript
    Set-Content -LiteralPath (Join-Path $stagingRoot "unlisted.dll") -Value "not a PE file"
    Invoke-CheckedPowerShell -ScriptPath $stageScript
    if (Test-Path -LiteralPath (Join-Path $stagingRoot "unlisted.dll")) {
        throw "Staging did not remove an unlisted file"
    }

    $mismatchManifestPath = Join-Path $temporaryRoot "manifest-hash-mismatch.json"
    $mismatchManifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json -Depth 20
    $mismatchManifest.files[0].sha256 = "0" * 64
    $mismatchManifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $mismatchManifestPath -Encoding utf8NoBOM
    Invoke-ExpectedFailure `
        -ScriptPath $stageScript `
        -Arguments @("-ManifestPath", $mismatchManifestPath, "-StagingRoot", $hashMismatchStaging) `
        -ExpectedMessage "Runtime source hash mismatch"

    $executableName = Split-Path -Leaf $resolvedExecutable
    $positiveFixturePath = Join-Path $temporaryRoot "imports-positive.json"
    @{ $executableName = @("VCRUNTIME140.dll", "KERNEL32.dll") } |
        ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath $positiveFixturePath -Encoding utf8NoBOM
    $positiveEvidencePath = Join-Path $temporaryRoot "imports-positive-evidence.json"
    $importScript = Join-Path $repositoryRoot "tools\validate-runtime-imports.ps1"
    Invoke-CheckedPowerShell `
        -ScriptPath $importScript `
        -Arguments @("-ExecutablePath", $resolvedExecutable, "-ImportFixturePath", $positiveFixturePath, "-EvidencePath", $positiveEvidencePath)
    $positiveEvidence = Get-Content -Raw -LiteralPath $positiveEvidencePath | ConvertFrom-Json -Depth 20
    $executableEvidence = @($positiveEvidence.files | Where-Object name -EQ $executableName)[0]
    $vcRuntimeImport = @($executableEvidence.imports | Where-Object name -EQ "VCRUNTIME140.dll")[0]
    if ($vcRuntimeImport.origin -ne "staging") {
        throw "MSVC runtime import was not resolved from staging"
    }

    $negativeFixturePath = Join-Path $temporaryRoot "imports-missing-vc-runtime.json"
    @{ $executableName = @("MSVCP140_2.dll") } |
        ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath $negativeFixturePath -Encoding utf8NoBOM
    Invoke-ExpectedFailure `
        -ScriptPath $importScript `
        -Arguments @("-ExecutablePath", $resolvedExecutable, "-ImportFixturePath", $negativeFixturePath, "-EvidencePath", (Join-Path $temporaryRoot "imports-negative-evidence.json")) `
        -ExpectedMessage "Required app-local MSVC runtime import 'MSVCP140_2.dll' is missing from staging"

    Write-Output "Runtime manifest, staging, and app-local MSVC import validation verified."
} finally {
    if (Test-Path -LiteralPath $hashMismatchStaging) {
        $resolvedHashMismatchStaging = [System.IO.Path]::GetFullPath($hashMismatchStaging)
        $runtimeRoot = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot "runtime"))
        if ($resolvedHashMismatchStaging.StartsWith("$runtimeRoot\", [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedHashMismatchStaging -Recurse -Force
        }
    }
    if (Test-Path -LiteralPath $temporaryRoot) {
        $resolvedTemporaryRoot = [System.IO.Path]::GetFullPath($temporaryRoot)
        $systemTemporaryRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if ($resolvedTemporaryRoot.StartsWith($systemTemporaryRoot, [StringComparison]::OrdinalIgnoreCase)) {
            Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
        }
    }
}
