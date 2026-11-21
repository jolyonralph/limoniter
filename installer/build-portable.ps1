# Builds the portable download for Windows: target\portable\Limoniter-<version>-Windows-portable.zip
#
#   powershell -ExecutionPolicy Bypass -File installer\build-portable.ps1
#
# Run it after build-installer.ps1, which builds the program and puts the licence notices together. The zip holds
# a folder, Limoniter-<version>, with limoniter.exe, its licences, and a file named "portable": with that beside
# it, Limoniter keeps its settings and everything else in a "Limoniter data" folder beside it, and nothing on the
# PC. Several versions can be unpacked side by side.

$ErrorActionPreference = 'Stop'
$app = Split-Path -Parent $PSScriptRoot
Set-Location $app

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$program = Join-Path $app 'target\release\limoniter.exe'
$notices = Join-Path $app 'target\installer\THIRD-PARTY-NOTICES.txt'
foreach ($needed in @($program, $notices)) {
    if (-not (Test-Path $needed)) { throw "$needed isn't there: run installer\build-installer.ps1 first." }
}

$output = Join-Path $app 'target\portable'
$folder = Join-Path $output "Limoniter-$version"
if (Test-Path $folder) { Remove-Item -Recurse -Force $folder }
New-Item -ItemType Directory -Force $folder | Out-Null
Copy-Item $program $folder
Copy-Item (Join-Path $app 'LICENSE') (Join-Path $folder 'LICENSE.txt')
Copy-Item $notices $folder

# The file that makes it portable. What it says is for whoever opens it; Limoniter only looks for it.
$words = @(
    'This file makes Limoniter portable.',
    '',
    'While it is beside limoniter.exe, Limoniter keeps its settings, recent files, layouts, the emulators and',
    'tools it downloads, and everything else in the "Limoniter data" folder beside it, and leaves nothing on',
    'this PC. Delete this file to keep them in your Windows account instead, as the installed Limoniter does.',
    '',
    'This copy is not signed yet, so Windows may say it protected your PC the first time: choose More info,',
    'then Run anyway.'
)
[IO.File]::WriteAllLines((Join-Path $folder 'portable'), $words)

# Each file named with forward slashes, as zips should be: Windows PowerShell's Compress-Archive writes backslashes,
# which some unzippers take as part of the name.
$zip = Join-Path $output "Limoniter-$version-Windows-portable.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($zip, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in Get-ChildItem -File $folder) {
        [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, "Limoniter-$version/$($file.Name)", [IO.Compression.CompressionLevel]::Optimal) | Out-Null
    }
} finally {
    $archive.Dispose()
}
$size = (Get-Item $zip).Length / 1MB
Write-Host ('Made {0} ({1:N1} MB)' -f $zip, $size)
