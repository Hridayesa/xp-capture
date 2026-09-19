Set-StrictMode -Version Latest

$Script:RuntimeManifestSchemaVersion = 1

function New-RuntimeManifestFile {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Sha256,
        [Parameter(Mandatory)][ValidateSet("x64")][string]$Architecture,
        [Parameter(Mandatory)][string]$Source,
        [Parameter(Mandatory)][string]$BundleDestination,
        [Parameter(Mandatory)][ValidateSet("opencv", "codec", "transitive")][string]$Purpose,
        [Parameter(Mandatory)][string]$LicenseNoticePath
    )

    [ordered]@{
        name = $Name
        sha256 = $Sha256.ToLowerInvariant()
        architecture = $Architecture
        source = $Source
        bundleDestination = $BundleDestination
        purpose = $Purpose
        licenseNoticePath = $LicenseNoticePath
    }
}

function New-RuntimeManifest {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$VcpkgRevision,
        [Parameter(Mandatory)][object[]]$Files
    )

    [ordered]@{
        schemaVersion = $Script:RuntimeManifestSchemaVersion
        generatedAtUtc = [DateTimeOffset]::UtcNow.ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ss.fffffffZ")
        vcpkgRevision = $VcpkgRevision.ToLowerInvariant()
        triplet = "x64-windows"
        files = @($Files)
    }
}

function Assert-RuntimeManifestModel {
    [CmdletBinding()]
    param([Parameter(Mandatory)][object]$Manifest)

    if ($Manifest.schemaVersion -ne $Script:RuntimeManifestSchemaVersion) {
        throw "Unsupported runtime manifest schema version: $($Manifest.schemaVersion)"
    }
    if ($Manifest.triplet -ne "x64-windows") {
        throw "Unsupported runtime manifest triplet: $($Manifest.triplet)"
    }
    if ($Manifest.vcpkgRevision -notmatch '^[0-9a-f]{40}$') {
        throw "Runtime manifest vcpkgRevision must be a lowercase 40-character Git commit"
    }
    if (@($Manifest.files).Count -eq 0) {
        throw "Runtime manifest must contain at least one file"
    }

    foreach ($file in @($Manifest.files)) {
        if ($file.name -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]*\.dll$') {
            throw "Invalid runtime file name: $($file.name)"
        }
        if ($file.sha256 -notmatch '^[0-9a-f]{64}$') {
            throw "Invalid SHA-256 for runtime file: $($file.name)"
        }
        if ($file.architecture -ne "x64") {
            throw "Invalid PE architecture for runtime file $($file.name): $($file.architecture)"
        }
        if (-not $file.licenseNoticePath) {
            throw "Missing license/notice path for runtime file: $($file.name)"
        }
        foreach ($pathValue in @($file.source, $file.bundleDestination, $file.licenseNoticePath)) {
            if ($pathValue -match '[*?]' -or $pathValue -match '(^|/)\.\.(/|$)' -or $pathValue.Contains('\')) {
                throw "Runtime manifest paths must be explicit repository-relative POSIX paths: $pathValue"
            }
        }
    }

    $Manifest
}

Export-ModuleMember -Function New-RuntimeManifestFile, New-RuntimeManifest, Assert-RuntimeManifestModel
