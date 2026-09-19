[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet("Dev", "Build")][string]$Mode
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$runner = Join-Path $PSScriptRoot "run-with-opencv.ps1"

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

Push-Location $repositoryRoot
try {
    Invoke-Checked -FilePath bun -Arguments @("run", "validate:runtime-manifest", "--", (Join-Path $repositoryRoot "runtime\manifest.json"))
    Invoke-Checked -FilePath pwsh -Arguments @("-NoProfile", "-File", (Join-Path $PSScriptRoot "stage-runtime.ps1"))

    if ($Mode -eq "Dev") {
        Invoke-Checked -FilePath pwsh -Arguments @("-NoProfile", "-File", $runner, "bun", "run", "tauri", "dev")
        return
    }

    $previousEvidencePath = $env:XP_CAPTURE_BUILD_ENV_EVIDENCE
    try {
        $env:XP_CAPTURE_BUILD_ENV_EVIDENCE = Join-Path $repositoryRoot "evidence\tauri-build-environment.json"
        Invoke-Checked -FilePath pwsh -Arguments @("-NoProfile", "-File", $runner, "bun", "run", "tauri", "build", "--bundles", "nsis", "--ci")
    } finally {
        $env:XP_CAPTURE_BUILD_ENV_EVIDENCE = $previousEvidencePath
    }

    $installerRoot = Join-Path $repositoryRoot "target\release\bundle\nsis"
    $installers = @(Get-ChildItem -LiteralPath $installerRoot -File -Filter "*.exe" -ErrorAction SilentlyContinue)
    if ($installers.Count -ne 1) {
        throw "Expected exactly one NSIS artifact in $installerRoot, found $($installers.Count)"
    }
    Write-Output "Tauri NSIS build complete: $($installers[0].FullName)"
} finally {
    Pop-Location
}
