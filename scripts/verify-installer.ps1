# Sanity-checks a built CompanyIsland NSIS installer.
#
# Usage:  powershell -File scripts/verify-installer.ps1 <path-to-CompanyIsland_<version>_x64-setup.exe>
#
# Prints size and SHA256, confirms the file is a PE executable, and checks the file name follows
# CompanyIsland_<version>_x64-setup.exe with <version> equal to package.json. Exits 1 on any failure.
# NSIS installers are 32-bit PE stubs even when the payload is x64, so both x86 and x64 PE pass; the
# x64 payload is expressed by the file name. Inspect CompanyIsland.exe inside (7z x) for its own machine type.
# It also checks that Tauri's generated installer.nsi (next to the setup's bundle folder) lists the Island Center
# (centerCompanyIsland.Center.exe, its .pri and web	our.html).
# It does not check the Authenticode signature; see docs/INSTALLER.md for signing.
param([Parameter(Mandatory = $true)][string]$Path)

$ErrorActionPreference = 'Stop'
$failed = $false

if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
    Write-Error "File not found: $Path"
    exit 1
}
$file = Get-Item -LiteralPath $Path

$hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
'{0,-10} {1}' -f 'File', $file.FullName
'{0,-10} {1:N0} bytes ({2:N1} MiB)' -f 'Size', $file.Length, ($file.Length / 1MB)
'{0,-10} {1}' -f 'SHA256', $hash

# PE header: "MZ" at 0, e_lfanew at 0x3C, "PE\0\0" there, then IMAGE_FILE_HEADER.Machine (0x8664 = x64).
$stream = [System.IO.File]::OpenRead($file.FullName)
try {
    $reader = New-Object System.IO.BinaryReader $stream
    $isPe = $false
    $machine = 0
    if ($stream.Length -ge 0x40 -and $reader.ReadUInt16() -eq 0x5A4D) {
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -gt 0 -and $peOffset + 6 -le $stream.Length) {
            $stream.Position = $peOffset
            if ($reader.ReadUInt32() -eq 0x00004550) {
                $isPe = $true
                $machine = $reader.ReadUInt16()
            }
        }
    }
} finally {
    $stream.Dispose()
}

if ($isPe -and $machine -eq 0x8664) {
    '{0,-10} PE, x64 (machine 0x8664)' -f 'Format'
} elseif ($isPe -and $machine -eq 0x014C) {
    '{0,-10} PE, x86 NSIS stub (machine 0x014C), expected for Tauri NSIS' -f 'Format'
} elseif ($isPe) {
    '{0,-10} PE but unexpected machine 0x{1:X4}' -f 'Format', $machine
    $failed = $true
} else {
    '{0,-10} not a PE executable' -f 'Format'
    $failed = $true
}

$packageJson = Join-Path $PSScriptRoot '..\package.json'
$version = (Get-Content -LiteralPath $packageJson -Raw | ConvertFrom-Json).version
$expected = "CompanyIsland_${version}_x64-setup.exe"
if ($file.Name -ceq $expected) {
    '{0,-10} {1} (matches)' -f 'Name', $file.Name
} else {
    '{0,-10} {1}, expected {2}' -f 'Name', $file.Name, $expected
    $failed = $true
}

$file = Get-Item -LiteralPath $Path
# <target>\release\bundle\nsis\<setup>.exe -> <target>\release\nsis\x64\installer.nsi
$nsi = Join-Path $file.Directory.Parent.Parent.FullName 'nsis\x64\installer.nsi'
& node (Join-Path $PSScriptRoot 'build-center.cjs') --check-installer-script --nsi $nsi
if ($LASTEXITCODE -ne 0) { $failed = $true }

if ($failed) {
    Write-Output 'RESULT: FAILED'
    exit 1
}
Write-Output 'RESULT: OK'
