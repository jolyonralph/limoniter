# Builds the Windows installer: target\installer\Limoniter-Setup-<version>.exe
#
#   powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1
#
# It needs Rust and Python, and fetches its own copy of the Inno Setup compiler into target\, installing nothing on
# this computer. GitHub runs this same script for a release, as set out in .github\workflows\release.yml.

$ErrorActionPreference = 'Stop'
$app = Split-Path -Parent $PSScriptRoot
Set-Location $app

# The exact tool, checked by its fingerprint, so that nothing else can end up inside the installer.
$toolVersion = '6.7.3'
$toolHash = 'F780898E402FF80612CC8D9FCB8C6E02932BD1CB4C900FFDAA31F9341CFB49F4'

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
Write-Host "Building Limoniter $version"

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'The program did not build.' }

# A fresh PC may not have Microsoft's runtime, so the program mustn't need it. See .cargo\config.toml.
$program = [IO.File]::ReadAllBytes((Join-Path $app 'target\release\limoniter.exe'))
if ([Text.Encoding]::ASCII.GetString($program).ToLower().Contains('vcruntime140')) { throw 'The program still needs VCRUNTIME140.dll, which a fresh PC may not have.' }

$output = Join-Path $app 'target\installer'
New-Item -ItemType Directory -Force $output | Out-Null
python -I (Join-Path $app 'tools\gen_notices.py') $app (Join-Path $output 'THIRD-PARTY-NOTICES.txt')
if ($LASTEXITCODE -ne 0) { throw 'The licence notices could not be put together.' }

$tools = Join-Path $app "target\installer-tool\$toolVersion"
$compiler = Join-Path $tools 'tools\ISCC.exe'
if (-not (Test-Path $compiler)) {
    New-Item -ItemType Directory -Force $tools | Out-Null
    $package = Join-Path $tools 'tool.nupkg'
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/tools.innosetup/$toolVersion/tools.innosetup.$toolVersion.nupkg" -OutFile $package -UseBasicParsing
    if ((Get-FileHash $package -Algorithm SHA256).Hash -ne $toolHash) { throw 'The Inno Setup download is not the expected file, so it has not been used.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($package, $tools)
}

& $compiler "/DAppVersion=$version" /Qp (Join-Path $PSScriptRoot 'limoniter.iss')
if ($LASTEXITCODE -ne 0) { throw 'The installer did not build.' }

$built = Join-Path $output "Limoniter-Setup-$version.exe"
$size = (Get-Item $built).Length / 1MB
Write-Host ('Made {0} ({1:N1} MB)' -f $built, $size)
