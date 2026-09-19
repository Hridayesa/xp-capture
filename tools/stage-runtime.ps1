[CmdletBinding()]
param(
    [string]$ManifestPath = "",
    [string]$StagingRoot = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "runtime-manifest.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "pe-tools.psm1") -Force

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $ManifestPath) {
    $ManifestPath = Join-Path $repositoryRoot "runtime\manifest.json"
}
if (-not $StagingRoot) {
    $StagingRoot = Join-Path $repositoryRoot "runtime\staging"
}
$resolvedManifestPath = [System.IO.Path]::GetFullPath($ManifestPath, $repositoryRoot)
$resolvedStagingRoot = [System.IO.Path]::GetFullPath($StagingRoot, $repositoryRoot)
$allowedStagingParent = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot "runtime"))
if (-not $resolvedStagingRoot.StartsWith("$allowedStagingParent\", [StringComparison]::OrdinalIgnoreCase)) {
    throw "Staging root must stay inside the repository runtime directory: $resolvedStagingRoot"
}

& bun run validate:runtime-manifest -- $resolvedManifestPath
if ($LASTEXITCODE -ne 0) {
    throw "Runtime manifest failed JSON Schema validation"
}
$manifest = Get-Content -Raw -LiteralPath $resolvedManifestPath | ConvertFrom-Json -Depth 20
Assert-RuntimeManifestModel -Manifest $manifest | Out-Null

$verified = foreach ($file in $manifest.files) {
    $sourcePath = [System.IO.Path]::GetFullPath(($file.source -replace '/', '\'), $repositoryRoot)
    if (-not $sourcePath.StartsWith("$repositoryRoot\", [StringComparison]::OrdinalIgnoreCase)) {
        throw "Runtime source escapes repository root: $($file.source)"
    }
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Runtime source is missing: $sourcePath"
    }
    $actualHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $file.sha256) {
        throw "Runtime source hash mismatch for $($file.name). Expected $($file.sha256), actual $actualHash"
    }
    $actualArchitecture = Get-PeArchitecture -Path $sourcePath
    if ($actualArchitecture -ne $file.architecture) {
        throw "Runtime source architecture mismatch for $($file.name). Expected $($file.architecture), actual $actualArchitecture"
    }
    [ordered]@{ manifest = $file; sourcePath = $sourcePath }
}

if (Test-Path -LiteralPath $resolvedStagingRoot) {
    Remove-Item -LiteralPath $resolvedStagingRoot -Recurse -Force
}
New-Item -ItemType Directory -Path $resolvedStagingRoot | Out-Null

foreach ($item in $verified) {
    $destinationRelative = $item.manifest.bundleDestination -replace '^runtime/', ''
    $destination = Join-Path $resolvedStagingRoot $destinationRelative
    Copy-Item -LiteralPath $item.sourcePath -Destination $destination
}

$expectedNames = @($manifest.files.name | Sort-Object)
$actualNames = @(Get-ChildItem -LiteralPath $resolvedStagingRoot -File | Select-Object -ExpandProperty Name | Sort-Object)
if (Compare-Object -ReferenceObject $expectedNames -DifferenceObject $actualNames) {
    throw "Staging output differs from runtime manifest allowlist"
}

Write-Output "Runtime staging complete with $($actualNames.Count) allowlisted file(s): $resolvedStagingRoot"
