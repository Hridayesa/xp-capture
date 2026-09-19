[CmdletBinding()]
param(
    [string]$InstallerPath = "",
    [string]$EvidencePath = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

Import-Module (Join-Path $PSScriptRoot "pe-tools.psm1") -Force

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if (-not $EvidencePath) {
    $EvidencePath = Join-Path $repositoryRoot "evidence\bundle-verification.json"
}
$resolvedEvidencePath = [System.IO.Path]::GetFullPath($EvidencePath, $repositoryRoot)
$manifestPath = Join-Path $repositoryRoot "runtime\manifest.json"
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json -Depth 20

if (-not $InstallerPath) {
    $installerRoot = Join-Path $repositoryRoot "target\release\bundle\nsis"
    $installers = @(Get-ChildItem -LiteralPath $installerRoot -File -Filter "*.exe" -ErrorAction SilentlyContinue)
    if ($installers.Count -ne 1) {
        throw "Expected exactly one NSIS installer in $installerRoot, found $($installers.Count)"
    }
    $InstallerPath = $installers[0].FullName
}
$resolvedInstaller = [System.IO.Path]::GetFullPath($InstallerPath, $repositoryRoot)
if (-not (Test-Path -LiteralPath $resolvedInstaller -PathType Leaf)) {
    throw "NSIS installer is missing: $resolvedInstaller"
}

$verificationId = [Guid]::NewGuid().ToString("N")
$installationRoot = Join-Path $repositoryRoot "runtime\verification\$verificationId"
$selfCheckReportPath = Join-Path $repositoryRoot "evidence\artifacts\self-check-$verificationId.json"
$stageNames = @("manifest", "install", "headless_self_check", "module_provenance", "gui_smoke", "uninstall")
$stages = @($stageNames | ForEach-Object {
    [ordered]@{ name = $_; status = "skipped"; exitCode = $null; summary = "Stage was not reached." }
})
$installerItem = Get-Item -LiteralPath $resolvedInstaller
$evidence = [ordered]@{
    schemaVersion = 1
    generatedAtUtc = [DateTimeOffset]::UtcNow.ToString("O")
    status = "failed"
    installer = [ordered]@{
        path = $resolvedInstaller
        size = $installerItem.Length
        sha256 = (Get-FileHash -LiteralPath $resolvedInstaller -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    installation = [ordered]@{
        root = $null
        executable = $null
        executableSha256 = $null
        selfCheckReport = $null
    }
    stages = $stages
    loadedModules = @()
}

function Set-Stage {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][ValidateSet("passed", "failed", "skipped")][string]$Status,
        [AllowNull()][Nullable[int]]$ExitCode,
        [Parameter(Mandatory)][string]$Summary
    )

    $stage = $evidence.stages | Where-Object name -EQ $Name | Select-Object -First 1
    $stage.status = $Status
    $stage.exitCode = $ExitCode
    $stage.summary = $Summary
}

function Write-Evidence {
    $directory = Split-Path -Parent $resolvedEvidencePath
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $temporary = "$resolvedEvidencePath.tmp"
    $evidence | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $temporary -Encoding utf8NoBOM
    Move-Item -LiteralPath $temporary -Destination $resolvedEvidencePath -Force
}

function Invoke-Process {
    param(
        [Parameter(Mandatory)][string]$FilePath,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory = $repositoryRoot,
        [switch]$CleanRuntimeEnvironment
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    foreach ($argument in $Arguments) {
        [void]$startInfo.ArgumentList.Add($argument)
    }
    if ($CleanRuntimeEnvironment) {
        foreach ($key in @($startInfo.Environment.Keys)) {
            if ($key -like "OPENCV_*" -or $key -like "VCPKG_*") {
                [void]$startInfo.Environment.Remove($key)
            }
        }
        $windowsRoot = [System.IO.Path]::GetFullPath($env:WINDIR)
        $startInfo.Environment["PATH"] = "$(Join-Path $windowsRoot 'System32');$windowsRoot"
    }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Failed to start process: $FilePath"
    }
    $process.WaitForExit()
    $process.ExitCode
}

function Get-NormalizedWindowsPath {
    param([Parameter(Mandatory)][string]$Path)

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    if ($fullPath.StartsWith('\\?\', [StringComparison]::Ordinal)) {
        $fullPath = $fullPath.Substring(4)
    }
    $fullPath.TrimEnd('\').ToLowerInvariant()
}

$installedExecutable = $null
$installationSucceeded = $false
$failure = $null
try {
    & bun run validate:runtime-manifest -- $manifestPath
    if ($LASTEXITCODE -ne 0) {
        throw "Runtime manifest schema validation failed"
    }
    foreach ($file in $manifest.files) {
        $source = [System.IO.Path]::GetFullPath(($file.source -replace '/', '\'), $repositoryRoot)
        $license = [System.IO.Path]::GetFullPath(($file.licenseNoticePath -replace '/', '\'), $repositoryRoot)
        if (-not (Test-Path -LiteralPath $source -PathType Leaf) -or
            -not (Test-Path -LiteralPath $license -PathType Leaf) -or
            (Get-PeArchitecture -Path $source) -ne "x64" -or
            (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -ne $file.sha256) {
            throw "Runtime manifest entry failed source/hash/architecture/license validation: $($file.name)"
        }
    }
    Set-Stage -Name manifest -Status passed -ExitCode 0 -Summary "Installer metadata and 15 manifest entries passed schema, hash, x64, and license validation."
    Write-Evidence

    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $installationRoot) | Out-Null
    if (Test-Path -LiteralPath $installationRoot) {
        throw "Fresh verification install path already exists: $installationRoot"
    }
    $installExitCode = Invoke-Process -FilePath $resolvedInstaller -Arguments @("/S", "/D=$installationRoot")
    if ($installExitCode -ne 0) {
        throw "NSIS installer exited with code $installExitCode"
    }
    $installationSucceeded = $true
    $executables = @(Get-ChildItem -LiteralPath $installationRoot -File -Recurse -Filter "xp-capture.exe")
    if ($executables.Count -ne 1) {
        throw "Expected one installed xp-capture.exe, found $($executables.Count)"
    }
    $installedExecutable = $executables[0].FullName
    foreach ($file in $manifest.files) {
        $installedFile = Join-Path $installationRoot ($file.bundleDestination -replace '/', '\')
        if (-not (Test-Path -LiteralPath $installedFile -PathType Leaf) -or
            (Get-FileHash -LiteralPath $installedFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $file.sha256 -or
            (Get-PeArchitecture -Path $installedFile) -ne "x64") {
            throw "Installed runtime payload does not match manifest: $($file.name)"
        }
    }
    $evidence.installation.root = $installationRoot
    $evidence.installation.executable = $installedExecutable
    $evidence.installation.executableSha256 = (Get-FileHash -LiteralPath $installedExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
    Set-Stage -Name install -Status passed -ExitCode $installExitCode -Summary "Current-user NSIS install produced one executable and exact manifest-listed runtime payload."
    Write-Evidence

    $selfCheckExitCode = Invoke-Process `
        -FilePath $installedExecutable `
        -Arguments @("--self-check", "--json", $selfCheckReportPath) `
        -WorkingDirectory $installationRoot `
        -CleanRuntimeEnvironment
    if ($selfCheckExitCode -ne 0 -or -not (Test-Path -LiteralPath $selfCheckReportPath -PathType Leaf)) {
        throw "Installed headless self-check failed with exit code $selfCheckExitCode"
    }
    $selfCheck = Get-Content -Raw -LiteralPath $selfCheckReportPath | ConvertFrom-Json -Depth 30
    if ($selfCheck.schema_version -ne 1 -or @($selfCheck.checks | Where-Object status -NE "passed").Count -gt 0) {
        throw "Installed self-check report contains failed or skipped checks"
    }
    $evidence.installation.selfCheckReport = $selfCheckReportPath
    Set-Stage -Name headless_self_check -Status passed -ExitCode $selfCheckExitCode -Summary "Installed headless self-check passed with system-only PATH and no OPENCV_*/VCPKG_* variables."
    Write-Evidence

    $expectedByName = @{}
    foreach ($file in $manifest.files) {
        $expectedByName[$file.name.ToLowerInvariant()] = $file
    }
    if (@($selfCheck.loaded_modules).Count -ne $expectedByName.Count) {
        throw "Loaded module count does not match the runtime manifest"
    }
    $moduleEvidence = @()
    foreach ($module in $selfCheck.loaded_modules) {
        $key = $module.name.ToLowerInvariant()
        if (-not $expectedByName.ContainsKey($key)) {
            throw "Self-check reported an unlisted runtime module: $($module.name)"
        }
        $expected = $expectedByName[$key]
        $expectedPath = [System.IO.Path]::GetFullPath((Join-Path $installationRoot ($expected.bundleDestination -replace '/', '\')))
        $actualPath = [System.IO.Path]::GetFullPath($module.canonical_path)
        if ((Get-NormalizedWindowsPath -Path $actualPath) -ne (Get-NormalizedWindowsPath -Path $expectedPath) -or
            $module.sha256 -ne $expected.sha256) {
            throw "Installed module provenance mismatch: $($module.name)"
        }
        $moduleEvidence += [ordered]@{ name = $module.name; path = $actualPath; sha256 = $module.sha256 }
    }
    $evidence.loadedModules = @($moduleEvidence | Sort-Object name)
    Set-Stage -Name module_provenance -Status passed -ExitCode 0 -Summary "All loaded native runtime modules match installed paths and manifest hashes."
    Write-Evidence

    $guiStartInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $guiStartInfo.FileName = $installedExecutable
    $guiStartInfo.WorkingDirectory = $installationRoot
    $guiStartInfo.UseShellExecute = $false
    foreach ($key in @($guiStartInfo.Environment.Keys)) {
        if ($key -like "OPENCV_*" -or $key -like "VCPKG_*") {
            [void]$guiStartInfo.Environment.Remove($key)
        }
    }
    $windowsRoot = [System.IO.Path]::GetFullPath($env:WINDIR)
    $guiStartInfo.Environment["PATH"] = "$(Join-Path $windowsRoot 'System32');$windowsRoot"
    $guiProcess = [System.Diagnostics.Process]::new()
    $guiProcess.StartInfo = $guiStartInfo
    if (-not $guiProcess.Start()) {
        throw "Installed GUI process did not start"
    }
    $windowObserved = $false
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(20)
    while ([DateTimeOffset]::UtcNow -lt $deadline -and -not $guiProcess.HasExited) {
        $guiProcess.Refresh()
        if ($guiProcess.MainWindowHandle -ne [IntPtr]::Zero) {
            $windowObserved = $true
            break
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not $windowObserved) {
        if (-not $guiProcess.HasExited) { $guiProcess.Kill($true) }
        throw "Installed GUI did not expose a main window within 20 seconds"
    }
    if (-not $guiProcess.CloseMainWindow() -or -not $guiProcess.WaitForExit(10000)) {
        if (-not $guiProcess.HasExited) { $guiProcess.Kill($true) }
        throw "Installed GUI did not shut down through CloseMainWindow"
    }
    Set-Stage -Name gui_smoke -Status passed -ExitCode $guiProcess.ExitCode -Summary "Installed GUI exposed a main window and completed controlled CloseMainWindow shutdown."
    Write-Evidence
} catch {
    $failure = $_.Exception.Message
    $current = $evidence.stages | Where-Object status -EQ "skipped" | Select-Object -First 1
    if ($current -and $current.name -ne "uninstall") {
        Set-Stage -Name $current.name -Status failed -ExitCode 1 -Summary $failure
    }
    Write-Evidence
} finally {
    if ($installationSucceeded) {
        try {
            $uninstallers = @(Get-ChildItem -LiteralPath $installationRoot -File -Recurse -Filter "uninstall.exe")
            if ($uninstallers.Count -ne 1) {
                throw "Expected one installed uninstaller, found $($uninstallers.Count)"
            }
            $uninstallExitCode = Invoke-Process -FilePath $uninstallers[0].FullName -Arguments @("/S") -WorkingDirectory $installationRoot
            if ($uninstallExitCode -ne 0) {
                throw "NSIS uninstaller exited with code $uninstallExitCode"
            }
            $uninstallDeadline = [DateTimeOffset]::UtcNow.AddSeconds(15)
            while ($installedExecutable -and
                (Test-Path -LiteralPath $installedExecutable) -and
                [DateTimeOffset]::UtcNow -lt $uninstallDeadline) {
                Start-Sleep -Milliseconds 250
            }
            if ($installedExecutable -and (Test-Path -LiteralPath $installedExecutable)) {
                throw "Installed executable remains after uninstall"
            }
            Set-Stage -Name uninstall -Status passed -ExitCode $uninstallExitCode -Summary "NSIS uninstall completed and removed the installed executable."
        } catch {
            Set-Stage -Name uninstall -Status failed -ExitCode 1 -Summary $_.Exception.Message
            if (-not $failure) { $failure = $_.Exception.Message }
        }
    } else {
        Set-Stage -Name uninstall -Status skipped -ExitCode $null -Summary "No successful installation was available to uninstall."
    }

    if (-not $failure -and @($evidence.stages | Where-Object status -NE "passed").Count -eq 0) {
        $evidence.status = "passed"
    }
    Write-Evidence
}

& bun (Join-Path $PSScriptRoot "validate-bundle-evidence.mjs") $resolvedEvidencePath
if ($LASTEXITCODE -ne 0) {
    throw "Bundle verification evidence failed schema validation"
}
if ($evidence.status -ne "passed") {
    throw "Bundle verification failed: $failure"
}

Write-Output "Installed bundle verification passed. Evidence: $resolvedEvidencePath"
