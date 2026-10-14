; Compile: ISCC /DAppVersion=0.1.0 /DBinaryDir=... packaging/windows.iss
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef BinaryDir
  #define BinaryDir "..\target\release"
#endif
[Setup]
AppId={{8B908BC1-B618-42A7-9DF4-5E269C59C90D}
AppName=LiteView
AppVersion={#AppVersion}
AppPublisher=Lianzy-Baimiao
AppPublisherURL=https://github.com/Lianzy-Baimiao/LiteView
DefaultDirName={localappdata}\Programs\LiteView
DefaultGroupName=LiteView
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=LiteView-v{#AppVersion}-windows-x64-setup
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\LiteView.exe
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "associate"; Description: "Register supported image types (does not change defaults)"

[Files]
Source: "{#BinaryDir}\LiteView.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD-PARTY-NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\INSTALL.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\LiteView"; Filename: "{app}\LiteView.exe"
Name: "{autodesktop}\LiteView"; Filename: "{app}\LiteView.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\LiteView.exe"; Parameters: "--register"; Tasks: associate; Flags: runhidden waituntilterminated
Filename: "{app}\LiteView.exe"; Description: "Launch LiteView"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\LiteView.exe"; Parameters: "--unregister"; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterLiteView"

; User pictures and configuration are intentionally never removed by the uninstaller.
