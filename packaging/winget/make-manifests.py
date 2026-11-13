"""Writes winget's three manifests for a release of Limoniter, ready for a pull request to microsoft/winget-pkgs:

    python packaging/winget/make-manifests.py VERSION SHA256 OUTPUT_FOLDER

SHA256 is the installer's, as published on the release. The manifests go in OUTPUT_FOLDER/manifests/l/Limoniter/
Limoniter/VERSION, laid out as winget-pkgs keeps them.
"""

import os
import sys

IDENTIFIER = "Limoniter.Limoniter"
SCHEMA = "1.6.0"
# The installer's AppId, as Inno Setup records it for uninstalling.
PRODUCT_CODE = "{AF460EC1-5B7A-40E8-BC08-4D143E4AEB91}_is1"


def manifests(version: str, sha256: str) -> dict:
    url = f"https://github.com/jolyonralph/limoniter/releases/download/v{version}/Limoniter-Setup-{version}.exe"
    header = lambda kind: f"# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{SCHEMA}.schema.json\n\n"
    return {
        f"{IDENTIFIER}.yaml": header("version") + f"""PackageIdentifier: {IDENTIFIER}
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: {SCHEMA}
""",
        f"{IDENTIFIER}.installer.yaml": header("installer") + f"""PackageIdentifier: {IDENTIFIER}
PackageVersion: {version}
InstallerType: inno
Scope: user
InstallModes:
  - interactive
  - silent
  - silentWithProgress
UpgradeBehavior: install
ProductCode: '{PRODUCT_CODE}'
MinimumOSVersion: 10.0.0.0
Installers:
  - Architecture: x64
    InstallerUrl: {url}
    InstallerSha256: {sha256.upper()}
ManifestType: installer
ManifestVersion: {SCHEMA}
""",
        f"{IDENTIFIER}.locale.en-US.yaml": header("defaultLocale") + f"""PackageIdentifier: {IDENTIFIER}
PackageVersion: {version}
PackageLocale: en-US
Publisher: Limoniter
PublisherUrl: https://limoniter.com
PublisherSupportUrl: https://github.com/jolyonralph/limoniter/issues
PackageName: Limoniter
PackageUrl: https://limoniter.com
License: Apache-2.0
LicenseUrl: https://github.com/jolyonralph/limoniter/blob/main/LICENSE
ShortDescription: A text and hex editor for everyday work, and for the files of older computers.
Description: |-
  Limoniter is a free, open-source text and hex editor: column editing, an undo tree, Find in Files, compare,
  code layout, Git, macros, scripts and 10 GB files. It also edits the files of older computers byte for byte
  (PETSCII, ATASCII, EBCDIC, teletext and many more character sets), opens their disks and tapes, lists their
  BASIC, runs programs on a built-in ZX Spectrum or an emulator, and sends work back to the real machine.
Moniker: limoniter
Tags:
  - editor
  - text-editor
  - hex-editor
  - git
  - retro
  - commodore
  - zx-spectrum
  - amiga
ReleaseNotesUrl: https://github.com/jolyonralph/limoniter/releases/tag/v{version}
ManifestType: defaultLocale
ManifestVersion: {SCHEMA}
""",
    }


def main() -> None:
    version, sha256, output = sys.argv[1:4]
    if len(sha256) != 64 or any(c not in "0123456789abcdefABCDEF" for c in sha256):
        sys.exit("The SHA256 should be 64 hex digits.")
    folder = os.path.join(output, "manifests", "l", "Limoniter", "Limoniter", version)
    os.makedirs(folder, exist_ok=True)
    for name, text in manifests(version, sha256).items():
        with open(os.path.join(folder, name), "w", encoding="utf-8", newline="\n") as file:
            file.write(text)
    print(folder)


if __name__ == "__main__":
    main()
