; The Windows installer, for Inno Setup. Don't run this by itself: build-installer.ps1 beside it does everything,
; and passes in the version from Cargo.toml.

#ifndef AppVersion
  #error Build this with installer\build-installer.ps1, which says which version it is.
#endif

[Setup]
; This identifies Limoniter to Windows for upgrades and uninstalling. It must never change.
AppId={{AF460EC1-5B7A-40E8-BC08-4D143E4AEB91}
AppName=Limoniter
AppVersion={#AppVersion}
AppVerName=Limoniter {#AppVersion}
AppPublisher=Limoniter
AppPublisherURL=https://github.com/jolyonralph/Limoniter
AppSupportURL=https://github.com/jolyonralph/Limoniter/issues
AppUpdatesURL=https://github.com/jolyonralph/Limoniter/releases
VersionInfoVersion={#AppVersion}
; Installs for this Windows user, so it needs no administrator password.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Limoniter
DefaultGroupName=Limoniter
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\data\limoniter.ico
UninstallDisplayIcon={app}\limoniter.exe
UninstallDisplayName=Limoniter
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Offers to close Limoniter if it's running while upgrading. It asks about unsaved work itself as it closes.
CloseApplications=yes
RestartApplications=no
OutputDir=..\target\installer
OutputBaseFilename=Limoniter-Setup-{#AppVersion}

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "..\target\release\limoniter.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\target\installer\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Limoniter"; Filename: "{app}\limoniter.exe"
Name: "{autodesktop}\Limoniter"; Filename: "{app}\limoniter.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\limoniter.exe"; Description: "Start Limoniter"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Takes back any file types this copy registered for, so Windows isn't left pointing at a program that's gone.
; Settings, macros, scripts and kept undo history stay, in case Limoniter is installed again.
Filename: "{app}\limoniter.exe"; Parameters: "--unregister-file-types"; RunOnceId: "UnregisterFileTypes"; Flags: runhidden waituntilterminated
