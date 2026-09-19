Set-StrictMode -Version Latest

function Get-MsvcDumpbinPath {
    $vsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vsWhere -PathType Leaf)) {
        throw "vswhere.exe is required to locate MSVC dumpbin"
    }
    $installationPath = (& $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
    if (-not $installationPath) {
        throw "MSVC x64 toolset is required to inspect PE imports"
    }
    $toolset = Get-ChildItem -LiteralPath (Join-Path $installationPath "VC\Tools\MSVC") -Directory |
        Sort-Object Name -Descending |
        Select-Object -First 1
    if (-not $toolset) {
        throw "MSVC toolset directory was not found"
    }
    $path = Join-Path $toolset.FullName "bin\Hostx64\x64\dumpbin.exe"
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "dumpbin.exe was not found: $path"
    }
    $path
}

function Get-PeArchitecture {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path)

    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
        $reader = [System.IO.BinaryReader]::new($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) {
            throw "Not a PE file: $Path"
        }
        $stream.Position = 0x3C
        $peOffset = $reader.ReadUInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "Invalid PE signature: $Path"
        }
        $machine = $reader.ReadUInt16()
        switch ($machine) {
            0x8664 { "x64" }
            0x014C { "x86" }
            0xAA64 { "arm64" }
            default { "unknown-0x{0:x4}" -f $machine }
        }
    } finally {
        $stream.Dispose()
    }
}

function Get-PeImports {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Dumpbin,
        [Parameter(Mandatory)][string]$Path
    )

    $output = & $Dumpbin /nologo /dependents $Path 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin failed for $Path"
    }
    @($output | ForEach-Object {
        $match = [regex]::Match($_, '^\s+([A-Za-z0-9._-]+\.dll)\s*$', [System.Text.RegularExpressions.RegexOptions]::IgnoreCase)
        if ($match.Success) { $match.Groups[1].Value }
    } | Sort-Object -Unique)
}

function Test-SystemImport {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Name)

    if ($Name -match '^(api-ms-win-|ext-ms-win-)') {
        return $true
    }
    $system32 = Join-Path $env:WINDIR "System32"
    Test-Path -LiteralPath (Join-Path $system32 $Name) -PathType Leaf
}

function Test-MsvcRuntimeImport {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Name)

    $Name -match '^(?:concrt140|msvcp140(?:_[12]|_atomic_wait|_codecvt_ids)?|vccorlib140|vcruntime140(?:_1|_threads)?)\.dll$'
}

Export-ModuleMember -Function Get-MsvcDumpbinPath, Get-PeArchitecture, Get-PeImports, Test-SystemImport, Test-MsvcRuntimeImport
