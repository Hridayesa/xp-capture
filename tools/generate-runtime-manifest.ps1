[CmdletBinding()]
param(
    [string]$InstallRoot = "",
    [string]$OutputPath = "",
    [string]$MsvcInstallationPath = "",
    [string[]]$RootDll = @(
        "opencv_core4.dll",
        "opencv_imgproc4.dll",
        "opencv_imgcodecs4.dll",
        "opencv_videoio4.dll"
    )
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "runtime-manifest.psm1") -Force
Import-Module (Join-Path $PSScriptRoot "pe-tools.psm1") -Force

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$pinnedRevision = "9e593bb18ea69cc5095e012465dcd675a822ed0d"
if (-not $InstallRoot) {
    $InstallRoot = Join-Path $repositoryRoot ".vcpkg_installed"
}
if (-not $OutputPath) {
    $OutputPath = Join-Path $repositoryRoot "runtime\manifest.json"
}
$resolvedInstallRoot = [System.IO.Path]::GetFullPath($InstallRoot, $repositoryRoot)
$releaseBin = Join-Path $resolvedInstallRoot "x64-windows\bin"
$infoRoot = Join-Path $resolvedInstallRoot "vcpkg\info"

function Get-MsvcRedistributable {
    if (-not $MsvcInstallationPath) {
        $vsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
        if (-not (Test-Path -LiteralPath $vsWhere -PathType Leaf)) {
            throw "vswhere.exe is required to locate the MSVC Redistributable"
        }
        $script:MsvcInstallationPath = (& $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
    }
    if (-not $MsvcInstallationPath) {
        throw "MSVC installation is required to locate the app-local VC runtime"
    }

    $versionFile = Join-Path $MsvcInstallationPath "VC\Auxiliary\Build\Microsoft.VCRedistVersion.default.txt"
    if (-not (Test-Path -LiteralPath $versionFile -PathType Leaf)) {
        throw "MSVC Redistributable version marker is missing: $versionFile"
    }
    $version = (Get-Content -Raw -LiteralPath $versionFile).Trim()
    if ($version -notmatch '^\d+\.\d+\.\d+$') {
        throw "Invalid MSVC Redistributable version in ${versionFile}: $version"
    }

    $crtRoot = Join-Path $MsvcInstallationPath "VC\Redist\MSVC\$version\x64\Microsoft.VC143.CRT"
    if (-not (Test-Path -LiteralPath $crtRoot -PathType Container)) {
        throw "MSVC x64 Redistributable CRT directory is missing: $crtRoot"
    }
    $licensePath = Join-Path $MsvcInstallationPath "Licenses\1033\Redist.txt"
    if (-not (Test-Path -LiteralPath $licensePath -PathType Leaf)) {
        throw "MSVC redistribution notice is missing: $licensePath"
    }

    [ordered]@{
        version = $version
        crtRoot = $crtRoot
        licensePath = $licensePath
    }
}

function Get-PackageOwnership {
    param([Parameter(Mandatory)][string]$RelativeInstalledPath)

    foreach ($listFile in Get-ChildItem -LiteralPath $infoRoot -File -Filter "*.list") {
        $owned = Select-String -LiteralPath $listFile.FullName -SimpleMatch $RelativeInstalledPath -Quiet
        if ($owned) {
            $match = [regex]::Match($listFile.Name, '^(?<name>.+?)_[^_]+_x64-windows\.list$')
            if (-not $match.Success) {
                throw "Cannot parse vcpkg package owner from $($listFile.Name)"
            }
            return $match.Groups['name'].Value
        }
    }
    throw "No vcpkg package owns $RelativeInstalledPath"
}

if (-not (Test-Path -LiteralPath $releaseBin -PathType Container)) {
    throw "Release vcpkg bin directory is missing: $releaseBin"
}
$dumpbin = Get-MsvcDumpbinPath
$msvcRedist = Get-MsvcRedistributable
$candidateByName = [System.Collections.Generic.Dictionary[string, object]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($file in Get-ChildItem -LiteralPath $releaseBin -File -Filter "*.dll") {
    $candidateByName[$file.Name] = [ordered]@{ path = $file.FullName; origin = "vcpkg" }
}
foreach ($file in Get-ChildItem -LiteralPath $msvcRedist.crtRoot -File -Filter "*.dll") {
    if (Test-MsvcRuntimeImport -Name $file.Name) {
        $candidateByName[$file.Name] = [ordered]@{ path = $file.FullName; origin = "msvc_redist" }
    }
}

$queue = [System.Collections.Generic.Queue[string]]::new()
foreach ($rootName in $RootDll) {
    if (-not $candidateByName.ContainsKey($rootName)) {
        throw "Required root DLL is missing from release install tree: $rootName"
    }
    $queue.Enqueue($rootName)
}

$closure = [System.Collections.Generic.Dictionary[string, object]]::new([StringComparer]::OrdinalIgnoreCase)
while ($queue.Count -gt 0) {
    $name = $queue.Dequeue()
    if ($closure.ContainsKey($name)) {
        continue
    }
    $candidate = $candidateByName[$name]
    $path = $candidate.path
    $closure[$name] = $candidate

    foreach ($dependency in Get-PeImports -Dumpbin $dumpbin -Path $path) {
        if ($candidateByName.ContainsKey($dependency)) {
            if (-not $closure.ContainsKey($dependency)) {
                $queue.Enqueue($dependency)
            }
        } elseif (Test-MsvcRuntimeImport -Name $dependency) {
            throw "Required MSVC runtime import '$dependency' is missing from $($msvcRedist.crtRoot)"
        } elseif (-not (Test-SystemImport -Name $dependency)) {
            throw "Unresolved non-system PE import '$dependency' required by '$name'"
        }
    }
}

$redistCacheRoot = Join-Path $repositoryRoot ".tools\msvc-redist\$($msvcRedist.version)"
$redistCacheCrt = Join-Path $redistCacheRoot "x64\Microsoft.VC143.CRT"
New-Item -ItemType Directory -Force -Path $redistCacheCrt | Out-Null
$redistNotice = Join-Path $redistCacheRoot "Redist.txt"
Copy-Item -LiteralPath $msvcRedist.licensePath -Destination $redistNotice -Force

$entries = foreach ($pair in ($closure.GetEnumerator() | Sort-Object Key)) {
    $candidate = $pair.Value
    $sourcePath = $candidate.path
    if ($candidate.origin -eq "msvc_redist") {
        $cachedPath = Join-Path $redistCacheCrt $pair.Key
        Copy-Item -LiteralPath $sourcePath -Destination $cachedPath -Force
        $sourcePath = $cachedPath
    }

    $architecture = Get-PeArchitecture -Path $sourcePath
    if ($architecture -ne "x64") {
        throw "Runtime DLL is not x64: $sourcePath ($architecture)"
    }
    if ($sourcePath -match '[\\/]debug[\\/]') {
        throw "Debug DLL cannot enter the runtime manifest: $sourcePath"
    }

    if ($candidate.origin -eq "msvc_redist") {
        $source = ".tools/msvc-redist/$($msvcRedist.version)/x64/Microsoft.VC143.CRT/$($pair.Key)"
        $licenseNoticePath = ".tools/msvc-redist/$($msvcRedist.version)/Redist.txt"
        $purpose = "transitive"
    } else {
        $relativeInstalled = "x64-windows/bin/$($pair.Key)"
        $owner = Get-PackageOwnership -RelativeInstalledPath $relativeInstalled
        $licensePath = Join-Path $resolvedInstallRoot "x64-windows\share\$owner\copyright"
        if (-not (Test-Path -LiteralPath $licensePath -PathType Leaf)) {
            throw "License/notice metadata is missing for $($pair.Key): $licensePath"
        }
        $source = ".vcpkg_installed/x64-windows/bin/$($pair.Key)"
        $licenseNoticePath = ".vcpkg_installed/x64-windows/share/$owner/copyright"
        $purpose = if ($owner -eq "opencv4") {
            "opencv"
        } elseif ($owner -in @("ffmpeg", "libjpeg-turbo")) {
            "codec"
        } else {
            "transitive"
        }
    }

    New-RuntimeManifestFile `
        -Name $pair.Key `
        -Sha256 (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash `
        -Architecture $architecture `
        -Source $source `
        -BundleDestination $pair.Key `
        -Purpose $purpose `
        -LicenseNoticePath $licenseNoticePath
}

$manifest = New-RuntimeManifest -VcpkgRevision $pinnedRevision -Files $entries
Assert-RuntimeManifestModel -Manifest $manifest | Out-Null

$resolvedOutputPath = [System.IO.Path]::GetFullPath($OutputPath, $repositoryRoot)
$outputDirectory = Split-Path -Parent $resolvedOutputPath
New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
$temporaryPath = "$resolvedOutputPath.tmp"
$manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $temporaryPath -Encoding utf8NoBOM

& bun run validate:runtime-manifest -- $temporaryPath
if ($LASTEXITCODE -ne 0) {
    throw "Generated runtime manifest failed JSON Schema validation"
}
Move-Item -LiteralPath $temporaryPath -Destination $resolvedOutputPath -Force

Write-Output "Runtime manifest generated with $($entries.Count) file(s): $resolvedOutputPath"
