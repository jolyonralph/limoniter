# Builds Limoniter's package for the Microsoft Store: target\msix\Limoniter-<version>.msix
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows-store\make-msix.ps1
#
# The Store signs it, so it isn't signed here. Its identity has to be the one Partner Center gives when the name is
# reserved (Product identity: Package/Identity/Name and Publisher, and the publisher's display name). Those come
# from LIMONITER_STORE_NAME, LIMONITER_STORE_PUBLISHER and LIMONITER_STORE_PUBLISHER_NAME; without them, stand-ins
# are used, which make a package to look at but not to upload. It needs the Windows SDK (makeappx and makepri) and
# a release build of Limoniter (installer\build-installer.ps1 makes one, with the licence notices).

$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$app = Split-Path -Parent (Split-Path -Parent $here)
Set-Location $app

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# The Store wants four numbers, the last of them 0.
$packageVersion = "$version.0"
$name = if ($env:LIMONITER_STORE_NAME) { $env:LIMONITER_STORE_NAME } else { 'Limoniter.Limoniter' }
$publisher = if ($env:LIMONITER_STORE_PUBLISHER) { $env:LIMONITER_STORE_PUBLISHER } else { 'CN=Limoniter' }
$publisherName = if ($env:LIMONITER_STORE_PUBLISHER_NAME) { $env:LIMONITER_STORE_PUBLISHER_NAME } else { 'Limoniter' }
if (-not $env:LIMONITER_STORE_NAME) { Write-Host 'No Store identity given: the package is made with stand-ins, to look at, not to upload.' }

# The SDK's newest tools.
$kits = 'C:\Program Files (x86)\Windows Kits\10\bin'
$sdk = Get-ChildItem $kits -Directory | Where-Object { $_.Name -match '^10\.' -and (Test-Path (Join-Path $_.FullName 'x64\makeappx.exe')) } | Sort-Object { [version]$_.Name } | Select-Object -Last 1
if (-not $sdk) { throw 'The Windows SDK (makeappx.exe) was not found.' }
$makeappx = Join-Path $sdk.FullName 'x64\makeappx.exe'
$makepri = Join-Path $sdk.FullName 'x64\makepri.exe'

# LIMONITER_EXE can name a build kept somewhere else.
$program = if ($env:LIMONITER_EXE) { $env:LIMONITER_EXE } else { Join-Path $app 'target\release\limoniter.exe' }
$notices = Join-Path $app 'target\installer\THIRD-PARTY-NOTICES.txt'
if (-not (Test-Path $notices)) {
    New-Item -ItemType Directory -Force (Split-Path $notices) | Out-Null
    python -I (Join-Path $app 'tools\gen_notices.py') $app $notices
}
if (-not (Test-Path $program) -or -not (Test-Path $notices)) { throw 'Build Limoniter first, with installer\build-installer.ps1.' }

# Everything that goes in the package, laid out as it will be installed.
$stage = Join-Path $app 'target\msix\stage'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $stage 'Assets') | Out-Null
Copy-Item $program $stage
Copy-Item (Join-Path $app 'LICENSE') (Join-Path $stage 'LICENSE.txt')
Copy-Item $notices $stage
Copy-Item (Join-Path $here 'Assets\*') (Join-Path $stage 'Assets')

# The file types, from file-types.txt.
$types = foreach ($line in Get-Content (Join-Path $here 'file-types.txt')) {
    if ($line -match '^\s*(#|$)') { continue }
    $group, $display, $endings = $line.Split('|')
    $files = ($endings.Split(' ', [StringSplitOptions]::RemoveEmptyEntries) | ForEach-Object { "              <uap:FileType>.$_</uap:FileType>" }) -join "`r`n"
    @"
        <uap:Extension Category="windows.fileTypeAssociation">
          <uap:FileTypeAssociation Name="$group">
            <uap:DisplayName>$display</uap:DisplayName>
            <uap:SupportedFileTypes>
$files
            </uap:SupportedFileTypes>
          </uap:FileTypeAssociation>
        </uap:Extension>
"@
}
$escape = { param($text) [Security.SecurityElement]::Escape($text) }
$manifest = (Get-Content (Join-Path $here 'AppxManifest.xml') -Raw).
    Replace('{{NAME}}', (& $escape $name)).
    Replace('{{PUBLISHER}}', (& $escape $publisher)).
    Replace('{{PUBLISHER_DISPLAY}}', (& $escape $publisherName)).
    Replace('{{VERSION}}', $packageVersion).
    Replace('{{FILE_TYPES}}', ($types -join "`r`n"))
[IO.File]::WriteAllText((Join-Path $stage 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

# The resource index, so Windows picks each icon's size for the screen.
$config = Join-Path $app 'target\msix\priconfig.xml'
& $makepri createconfig /cf $config /dq en-GB /pv 10.0.0 /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makepri could not make its configuration.' }
# One package holds every size, so the index isn't split into packages of its own.
$xml = [xml](Get-Content $config -Raw)
$packaging = $xml.SelectSingleNode('//packaging')
if ($packaging) { [void]$packaging.ParentNode.RemoveChild($packaging) }
$xml.Save($config)
& $makepri new /pr $stage /cf $config /of (Join-Path $stage 'resources.pri') /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makepri could not index the icons.' }

$built = Join-Path $app "target\msix\Limoniter-$version.msix"
& $makeappx pack /o /d $stage /p $built | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'makeappx could not make the package.' }
$size = (Get-Item $built).Length / 1MB
Write-Host ('Made {0} ({1:N1} MB), as {2} {3}' -f $built, $size, $name, $packageVersion)
