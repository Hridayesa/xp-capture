$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$Command = @($args)
if ($Command.Count -gt 0 -and $Command[0] -eq "--") {
    $Command = @($Command | Select-Object -Skip 1)
}
if ($Command.Count -eq 0) {
    Write-Error "A child command is required. Example: ./tools/run-with-opencv.ps1 -- cargo test"
    exit 2
}

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$vcpkgRoot = Join-Path $repositoryRoot ".tools\vcpkg"
$installRoot = Join-Path $repositoryRoot ".vcpkg_installed"
$tripletRoot = Join-Path $installRoot "x64-windows"
$binPath = Join-Path $tripletRoot "bin"
$includePath = Join-Path $tripletRoot "include"
$opencvIncludePath = Join-Path $includePath "opencv4"
$libPath = Join-Path $tripletRoot "lib"
$opencvConfigPath = Join-Path $tripletRoot "share\opencv4"

foreach ($requiredPath in @($vcpkgRoot, $binPath, $includePath, $opencvIncludePath, $libPath, $opencvConfigPath)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Container)) {
        Write-Error "Pinned OpenCV environment is incomplete: $requiredPath. Run ./tools/bootstrap-opencv.ps1 first."
        exit 3
    }
}

$executable = $Command[0]
$arguments = @($Command | Select-Object -Skip 1)
$resolvedExecutable = Get-Command $executable -ErrorAction SilentlyContinue
if (-not $resolvedExecutable) {
    Write-Error "Child command was not found: $executable"
    exit 4
}

$startInfo = [System.Diagnostics.ProcessStartInfo]::new()
$startInfo.FileName = $resolvedExecutable.Source
$startInfo.WorkingDirectory = (Get-Location).Path
$startInfo.UseShellExecute = $false
foreach ($argument in $arguments) {
    [void]$startInfo.ArgumentList.Add($argument)
}
$inheritedNativeKeys = @($startInfo.Environment.Keys | Where-Object {
    $_ -like "OPENCV_*" -or $_ -like "VCPKG_*"
})
foreach ($key in $inheritedNativeKeys) {
    [void]$startInfo.Environment.Remove($key)
}
$startInfo.Environment["VCPKG_ROOT"] = $vcpkgRoot
$startInfo.Environment["VCPKG_INSTALLED_DIR"] = $installRoot
$startInfo.Environment["VCPKG_DEFAULT_TRIPLET"] = "x64-windows"
$startInfo.Environment["OPENCV_DIR"] = $opencvConfigPath
$startInfo.Environment["OPENCV_INCLUDE_PATHS"] = $opencvIncludePath
$startInfo.Environment["OPENCV_LINK_PATHS"] = $libPath
$startInfo.Environment["OPENCV_LINK_LIBS"] = "opencv_core4,opencv_imgproc4,opencv_imgcodecs4,opencv_videoio4"
$startInfo.Environment["OPENCV_DISABLE_PROBES"] = "pkg_config,cmake,vcpkg_cmake,vcpkg"
$startInfo.Environment["VCPKGRS_DYNAMIC"] = "1"
$startInfo.Environment["VCPKGRS_TRIPLET"] = "x64-windows"
$startInfo.Environment["OPENCV_DLL_DIR"] = $binPath
$startInfo.Environment["PATH"] = "$binPath;$env:PATH"

$process = [System.Diagnostics.Process]::new()
$process.StartInfo = $startInfo
if (-not $process.Start()) {
    Write-Error "Failed to start child command: $executable"
    exit 5
}
$process.WaitForExit()
exit $process.ExitCode
