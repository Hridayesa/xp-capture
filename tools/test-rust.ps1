[CmdletBinding()]
param()

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
    Invoke-Checked -FilePath cargo -Arguments @("fmt", "--all", "--", "--check")
    Invoke-Checked -FilePath pwsh -Arguments @(
        "-NoProfile", "-File", $runner,
        "cargo", "clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings"
    )
    $previousIntegration = $env:XP_CAPTURE_OPENCV_INTEGRATION
    try {
        $env:XP_CAPTURE_OPENCV_INTEGRATION = "1"
        Invoke-Checked -FilePath pwsh -Arguments @(
            "-NoProfile", "-File", $runner,
            "cargo", "test", "--workspace", "--locked"
        )
    } finally {
        $env:XP_CAPTURE_OPENCV_INTEGRATION = $previousIntegration
    }
} finally {
    Pop-Location
}
