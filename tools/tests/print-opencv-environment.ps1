param([string]$OutputPath = "")

$output = [ordered]@{
    currentDirectory = (Get-Location).Path
    vcpkgRoot = $env:VCPKG_ROOT
    installedRoot = $env:VCPKG_INSTALLED_DIR
    triplet = $env:VCPKG_DEFAULT_TRIPLET
    opencvDirectory = $env:OPENCV_DIR
    inheritedOpenCvBin = $env:OPENCV_BIN
    includePath = $env:OPENCV_INCLUDE_PATHS
    linkPath = $env:OPENCV_LINK_PATHS
    linkLibraries = $env:OPENCV_LINK_LIBS
    disabledProbes = $env:OPENCV_DISABLE_PROBES
    dllDirectory = $env:OPENCV_DLL_DIR
    processPath = $env:PATH
}

$json = $output | ConvertTo-Json -Compress
if ($OutputPath) {
    Set-Content -LiteralPath $OutputPath -Value $json -Encoding utf8NoBOM
} else {
    Write-Output $json
}
