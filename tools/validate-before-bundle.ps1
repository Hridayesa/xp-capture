[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Invoke-Checked {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$Arguments = @()
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

$buildEvidencePath = Join-Path $repositoryRoot "evidence\tauri-build-environment.json"
if (-not (Test-Path -LiteralPath $buildEvidencePath -PathType Leaf)) {
    throw "Tauri build environment evidence is missing: $buildEvidencePath"
}
$buildEvidence = Get-Content -Raw -LiteralPath $buildEvidencePath | ConvertFrom-Json -Depth 10
if ($buildEvidence.staticVcruntime -ne $true -or
    $buildEvidence.target -ne "x86_64-pc-windows-msvc" -or
    $buildEvidence.tauriCrate -ne "2.11.5" -or
    $buildEvidence.tauriCli -ne "2.11.4") {
    throw "Tauri build environment evidence does not match the pinned Windows contract"
}

Invoke-Checked -FilePath bun -Arguments @("run", "validate:runtime-manifest", "--", (Join-Path $repositoryRoot "runtime\manifest.json"))
Invoke-Checked -FilePath pwsh -Arguments @("-NoProfile", "-File", (Join-Path $PSScriptRoot "stage-runtime.ps1"))
Invoke-Checked -FilePath pwsh -Arguments @(
    "-NoProfile",
    "-File",
    (Join-Path $PSScriptRoot "validate-runtime-imports.ps1"),
    "-ExecutablePath",
    (Join-Path $repositoryRoot "target\release\xp-capture.exe")
)

Write-Output "Pre-bundle runtime validation passed with STATIC_VCRUNTIME=true."
