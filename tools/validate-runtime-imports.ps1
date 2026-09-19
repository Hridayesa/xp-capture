[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ExecutablePath,
    [string]$ManifestPath = "",
    [string]$StagingRoot = "",
    [string]$EvidencePath = "",
    [string]$ImportFixturePath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "pe-tools.psm1") -Force

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $ManifestPath) {
    $ManifestPath = Join-Path $repositoryRoot "runtime\manifest.json"
}
if (-not $StagingRoot) {
    $StagingRoot = Join-Path $repositoryRoot "runtime\staging"
}
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repositoryRoot "evidence\runtime-imports.json"
}
$resolvedExecutable = [System.IO.Path]::GetFullPath($ExecutablePath, $repositoryRoot)
$resolvedStagingRoot = [System.IO.Path]::GetFullPath($StagingRoot, $repositoryRoot)
if (-not (Test-Path -LiteralPath $resolvedExecutable -PathType Leaf)) {
    throw "Release executable is missing: $resolvedExecutable"
}
if ((Get-PeArchitecture -Path $resolvedExecutable) -ne "x64") {
    throw "Release executable is not x64: $resolvedExecutable"
}

$manifest = Get-Content -Raw -LiteralPath ([System.IO.Path]::GetFullPath($ManifestPath, $repositoryRoot)) | ConvertFrom-Json -Depth 20
$stagedByName = [System.Collections.Generic.Dictionary[string, string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($file in $manifest.files) {
    $stagedPath = Join-Path $resolvedStagingRoot $file.name
    if (-not (Test-Path -LiteralPath $stagedPath -PathType Leaf)) {
        throw "Manifest-listed staged DLL is missing: $stagedPath"
    }
    $stagedByName[$file.name] = $stagedPath
}
$unlisted = @(Get-ChildItem -LiteralPath $resolvedStagingRoot -File | Where-Object { -not $stagedByName.ContainsKey($_.Name) })
if ($unlisted.Count -gt 0) {
    throw "Staging contains unlisted file(s): $($unlisted.Name -join ', ')"
}

$fixture = if ($ImportFixturePath) {
    Get-Content -Raw -LiteralPath $ImportFixturePath | ConvertFrom-Json -AsHashtable -Depth 20
} else {
    @{}
}
$dumpbin = Get-MsvcDumpbinPath
$roots = @($resolvedExecutable) + @($stagedByName.Values | Sort-Object)
$filesEvidence = @()

foreach ($root in $roots) {
    $rootName = Split-Path -Leaf $root
    $imports = if ($fixture.ContainsKey($rootName)) {
        @($fixture[$rootName])
    } else {
        @(Get-PeImports -Dumpbin $dumpbin -Path $root)
    }
    $resolvedImports = @()

    foreach ($import in $imports) {
        if ($stagedByName.ContainsKey($import)) {
            $resolvedImports += [ordered]@{ name = $import; origin = "staging"; path = $stagedByName[$import] }
            continue
        }
        if (Test-MsvcRuntimeImport -Name $import) {
            throw "Required app-local MSVC runtime import '$import' is missing from staging (required by '$rootName')"
        }
        if ($import -match '^(api-ms-win-|ext-ms-win-)') {
            $resolvedImports += [ordered]@{ name = $import; origin = "windows_api_set"; path = $null }
            continue
        }

        $system32 = [System.IO.Path]::GetFullPath((Join-Path $env:WINDIR "System32"))
        $systemPath = [System.IO.Path]::GetFullPath((Join-Path $system32 $import))
        if (-not $systemPath.StartsWith("$system32\", [StringComparison]::OrdinalIgnoreCase) -or
            -not (Test-Path -LiteralPath $systemPath -PathType Leaf)) {
            throw "Unresolved non-system import '$import' required by '$rootName'"
        }
        $resolvedImports += [ordered]@{ name = $import; origin = "windows_system32"; path = $systemPath }
    }

    $filesEvidence += [ordered]@{
        name = $rootName
        path = $root
        sha256 = (Get-FileHash -LiteralPath $root -Algorithm SHA256).Hash.ToLowerInvariant()
        imports = $resolvedImports
    }
}

$evidence = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    status = "passed"
    executable = $resolvedExecutable
    stagingRoot = $resolvedStagingRoot
    files = $filesEvidence
}
$resolvedEvidencePath = [System.IO.Path]::GetFullPath($EvidencePath, $repositoryRoot)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $resolvedEvidencePath) | Out-Null
$evidence | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $resolvedEvidencePath -Encoding utf8NoBOM

Write-Output "Runtime import closure verified for $($roots.Count) PE file(s). Evidence: $resolvedEvidencePath"
