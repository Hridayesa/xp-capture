[CmdletBinding()]
param(
    [string]$EvidencePath = "",
    [string]$ProbeFixturePath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function New-Check {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][bool]$Passed,
        [Parameter(Mandatory)][string]$Summary,
        [string]$Version = "",
        [string]$Path = ""
    )

    [ordered]@{
        name = $Name
        status = if ($Passed) { "passed" } else { "failed" }
        summary = $Summary
        version = if ($Version) { $Version } else { $null }
        path = if ($Path) { $Path } else { $null }
    }
}

function Invoke-VersionProbe {
    param(
        [Parameter(Mandatory)][string]$Command,
        [Parameter(Mandatory)][string[]]$Arguments
    )

    $resolved = Get-Command $Command -ErrorAction SilentlyContinue
    if (-not $resolved) {
        return [ordered]@{ found = $false; version = ""; path = "" }
    }

    $output = (& $resolved.Source @Arguments 2>&1 | Out-String).Trim()
    [ordered]@{ found = ($LASTEXITCODE -eq 0); version = $output; path = $resolved.Source }
}

function Get-LiveProbe {
    $osArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
    $git = Invoke-VersionProbe -Command "git" -Arguments @("--version")
    $rust = Invoke-VersionProbe -Command "rustc" -Arguments @("--version")
    $bun = Invoke-VersionProbe -Command "bun" -Arguments @("--version")
    $cmake = Invoke-VersionProbe -Command "cmake" -Arguments @("--version")
    $ninja = Invoke-VersionProbe -Command "ninja" -Arguments @("--version")

    $vsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $msvc = [ordered]@{ found = $false; version = ""; path = "" }
    if (Test-Path -LiteralPath $vsWhere -PathType Leaf) {
        $installationPath = (& $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
        if ($installationPath) {
            $msvcRoot = Join-Path $installationPath "VC\Tools\MSVC"
            $toolset = Get-ChildItem -LiteralPath $msvcRoot -Directory -ErrorAction SilentlyContinue |
                Sort-Object Name -Descending |
                Select-Object -First 1
            if ($toolset) {
                $clPath = Join-Path $toolset.FullName "bin\Hostx64\x64\cl.exe"
                $msvc = [ordered]@{
                    found = (Test-Path -LiteralPath $clPath -PathType Leaf)
                    version = $toolset.Name
                    path = $clPath
                }
            }
        }
    }

    $sdk = [ordered]@{ found = $false; version = ""; path = "" }
    $sdkRoot = (Get-ItemProperty -LiteralPath "HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots" -ErrorAction SilentlyContinue).KitsRoot10
    if ($sdkRoot) {
        $includeRoot = Join-Path $sdkRoot "Include"
        $sdkVersion = Get-ChildItem -LiteralPath $includeRoot -Directory -ErrorAction SilentlyContinue |
            Where-Object { Test-Path -LiteralPath (Join-Path $_.FullName "um\Windows.h") -PathType Leaf } |
            Sort-Object Name -Descending |
            Select-Object -First 1
        if ($sdkVersion) {
            $sdk = [ordered]@{ found = $true; version = $sdkVersion.Name; path = $sdkVersion.FullName }
        }
    }

    [ordered]@{
        os = [ordered]@{ isWindows = $IsWindows; architecture = $osArchitecture }
        powershell = [ordered]@{ found = $true; version = $PSVersionTable.PSVersion.ToString(); path = $PSHOME }
        git = $git
        msvc = $msvc
        windowsSdk = $sdk
        rust = $rust
        bun = $bun
        cmake = $cmake
        ninja = $ninja
    }
}

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repositoryRoot "evidence\environment.json"
}
$resolvedEvidencePath = [System.IO.Path]::GetFullPath($EvidencePath, $repositoryRoot)

$probe = if ($ProbeFixturePath) {
    Get-Content -Raw -LiteralPath $ProbeFixturePath | ConvertFrom-Json -Depth 20
} else {
    Get-LiveProbe
}

$package = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "package.json") | ConvertFrom-Json
$toolchain = Get-Content -Raw -LiteralPath (Join-Path $repositoryRoot "rust-toolchain.toml")
$expectedBun = $package.packageManager -replace '^bun@', ''
$expectedRustMatch = [regex]::Match($toolchain, 'channel\s*=\s*"([^"]+)"')
$expectedRust = if ($expectedRustMatch.Success) { $expectedRustMatch.Groups[1].Value } else { "" }

$checks = @(
    New-Check -Name "windows_x64" -Passed ($probe.os.isWindows -and $probe.os.architecture -eq "X64") -Summary "Windows x64 host" -Version $probe.os.architecture
    New-Check -Name "powershell" -Passed ($probe.powershell.found -and ([version]$probe.powershell.version).Major -ge 7) -Summary "PowerShell 7 or newer" -Version $probe.powershell.version -Path $probe.powershell.path
    New-Check -Name "git" -Passed $probe.git.found -Summary "Git is available" -Version $probe.git.version -Path $probe.git.path
    New-Check -Name "msvc_x64" -Passed $probe.msvc.found -Summary "MSVC x64 toolset is available" -Version $probe.msvc.version -Path $probe.msvc.path
    New-Check -Name "windows_sdk" -Passed $probe.windowsSdk.found -Summary "Windows SDK is available" -Version $probe.windowsSdk.version -Path $probe.windowsSdk.path
    New-Check -Name "rust" -Passed ($probe.rust.found -and $probe.rust.version -match ([regex]::Escape($expectedRust))) -Summary "Rust matches rust-toolchain.toml" -Version $probe.rust.version -Path $probe.rust.path
    New-Check -Name "bun" -Passed ($probe.bun.found -and $probe.bun.version -eq $expectedBun) -Summary "Bun matches packageManager" -Version $probe.bun.version -Path $probe.bun.path
    New-Check -Name "cmake" -Passed $probe.cmake.found -Summary "CMake is available" -Version $probe.cmake.version -Path $probe.cmake.path
    New-Check -Name "ninja" -Passed $probe.ninja.found -Summary "Ninja is available" -Version $probe.ninja.version -Path $probe.ninja.path
)

$failedChecks = @($checks | Where-Object { $_.status -eq "failed" })
$evidence = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    repositoryRoot = $repositoryRoot
    expected = [ordered]@{
        architecture = "X64"
        rust = $expectedRust
        bun = $expectedBun
    }
    status = if ($failedChecks.Count -eq 0) { "passed" } else { "failed" }
    checks = $checks
}

$evidenceDirectory = Split-Path -Parent $resolvedEvidencePath
New-Item -ItemType Directory -Force -Path $evidenceDirectory | Out-Null
$evidence | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $resolvedEvidencePath -Encoding utf8NoBOM

if ($failedChecks.Count -gt 0) {
    $names = ($failedChecks | ForEach-Object { $_.name }) -join ", "
    Write-Error "Environment verification failed: $names"
    exit 1
}

Write-Output "Environment verification passed. Evidence: $resolvedEvidencePath"
