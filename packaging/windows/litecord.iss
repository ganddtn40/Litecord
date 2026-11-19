; Litecord's Windows installer. scripts/package-windows.py compiles it with
; Inno Setup 7 under Wine, from the same files as the portable ZIP.
;
; It installs for the person running it, without administrator rights, and can
; start Litecord in the tray when they sign in. That is the Run key value
; Settings > Advanced > "Start when you sign in" writes (src/autostart.rs), so
; the installer and the setting turn the same thing on and off.

#ifndef AppVersion
  #error Compile through scripts/package-windows.py, which defines AppVersion, VersionNumber and SourceDir
#endif
; The full preview defines Edition and FileSuffix; the Discord release neither.
; Both share the AppId, so installing one replaces the other.
#ifdef Edition
  #define DisplayName "Litecord (" + Edition + ")"
#else
  #define DisplayName "Litecord"
  #define FileSuffix ""
#endif

[Setup]
; Windows knows an installed Litecord by this; never change it.
AppId={{A7C4E2B1-5D3F-4E8A-9B6C-1F2D3E4A5B60}
AppName=Litecord
AppVersion={#AppVersion}
AppVerName={#DisplayName} {#AppVersion}
AppPublisher=ganddtn40
AppPublisherURL=https://github.com/ganddtn40/litecord
AppSupportURL=https://github.com/ganddtn40/litecord/issues
AppUpdatesURL=https://github.com/ganddtn40/litecord/releases
VersionInfoVersion={#VersionNumber}
; Per user: %LOCALAPPDATA%\Programs\Litecord, with no UAC prompt.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Litecord
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=Litecord-{#AppVersion}{#FileSuffix}-windows-x64-setup
SetupIconFile=litecord.ico
UninstallDisplayIcon={app}\litecord.exe
UninstallDisplayName={#DisplayName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Litecord keeps running in the tray with its window closed, so an update or
; uninstall closes it rather than asking.
CloseApplications=force
RestartApplications=no
#ifdef LicenseFile
LicenseFile={#LicenseFile}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
english.StartOnSignIn=Start Litecord automatically when Windows starts
english.StartOnSignInGroup=Startup:

[Tasks]
Name: "startonsignin"; Description: "{cm:StartOnSignIn}"; GroupDescription: "{cm:StartOnSignInGroup}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; README.txt explains the ZIP; the installer needs no instructions.
Source: "{#SourceDir}\*"; Excludes: "README.txt"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Litecord"; Filename: "{app}\litecord.exe"
Name: "{autodesktop}\Litecord"; Filename: "{app}\litecord.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Litecord"; ValueData: """{app}\litecord.exe"" --background"; Tasks: startonsignin
; Chosen here, it starts even if Task Manager had turned it off before.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: none; ValueName: "Litecord"; Flags: deletevalue dontcreatekey; Tasks: startonsignin
; Litecord's own links open here; claiming Discord's stays the person's choice.
Root: HKCU; Subkey: "Software\Classes\litecord"; ValueType: string; ValueName: ""; ValueData: "URL:Litecord Protocol"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\litecord"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\litecord\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\litecord.exe"",0"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\litecord\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\litecord.exe"" --open-link ""%1"""; Flags: uninsdeletekey
; Settings keeps the app discord:// links opened before claiming them here;
; uninstalling gives them back (below) and drops what was kept.
Root: HKCU; Subkey: "Software\Litecord"; Flags: uninsdeletekeyifempty dontcreatekey
Root: HKCU; Subkey: "Software\Litecord\DiscordProtocol"; Flags: uninsdeletekey dontcreatekey
; Each start registers Litecord's name and icon for its notifications
; (src/notify.rs); uninstalling forgets them.
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\io.github.ganddtn40.Litecord"; Flags: uninsdeletekey dontcreatekey
; Uninstalling stops starting at sign-in, however it was turned on.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Litecord"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: none; ValueName: "Litecord"; Flags: uninsdeletevalue dontcreatekey

[Run]
Filename: "{app}\litecord.exe"; Description: "{cm:LaunchProgram,Litecord}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Before the files go: discord:// links open in the app that had them again,
; if Settings claimed them for this Litecord.
Filename: "{app}\litecord.exe"; Parameters: "--release-discord-links"; Flags: runhidden waituntilterminated; RunOnceId: "ReleaseDiscordLinks"
