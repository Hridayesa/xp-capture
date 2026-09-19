$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$wrapper = Join-Path $repositoryRoot "tools\run-with-opencv.ps1"
$childProbe = Join-Path $repositoryRoot "tools\tests\print-opencv-environment.ps1"
$outputDirectory = Join-Path $repositoryRoot ".tmp"
$outputPath = Join-Path $outputDirectory "run-with-opencv-environment.json"
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$parentSnapshot = [ordered]@{
    VCPKG_ROOT = $env:VCPKG_ROOT
    VCPKG_INSTALLED_DIR = $env:VCPKG_INSTALLED_DIR
    VCPKG_DEFAULT_TRIPLET = $env:VCPKG_DEFAULT_TRIPLET
    OPENCV_DIR = $env:OPENCV_DIR
    OPENCV_INCLUDE_PATHS = $env:OPENCV_INCLUDE_PATHS
    OPENCV_LINK_PATHS = $env:OPENCV_LINK_PATHS
    OPENCV_DLL_DIR = $env:OPENCV_DLL_DIR
    PATH = $env:PATH
}

Push-Location ([System.IO.Path]::GetTempPath())
try {
    & $wrapper pwsh -NoProfile -File $childProbe -OutputPath $outputPath
    if ($LASTEXITCODE -ne 0) {
        throw "Wrapper failed with exit code $LASTEXITCODE"
    }
    $child = Get-Content -Raw -LiteralPath $outputPath | ConvertFrom-Json
} finally {
    Pop-Location
}

if ($child.vcpkgRoot -ne (Join-Path $repositoryRoot ".tools\vcpkg")) {
    throw "VCPKG_ROOT was not resolved from repository root"
}
if ($child.triplet -ne "x64-windows") {
    throw "Unexpected triplet: $($child.triplet)"
}
if ($null -ne $child.inheritedOpenCvBin) {
    throw "Inherited OPENCV_BIN leaked into child environment"
}
$pathEntries = @($child.processPath -split ";")
if (-not ($pathEntries | Where-Object { $_ -eq $child.dllDirectory })) {
    throw "OpenCV DLL directory was not added to child PATH"
}

foreach ($name in $parentSnapshot.Keys) {
    $actual = [Environment]::GetEnvironmentVariable($name, "Process")
    if (($actual ?? "") -ne ($parentSnapshot[$name] ?? "")) {
        throw "Parent environment changed for $name"
    }
}

Write-Output "run-with-opencv process isolation verified"
