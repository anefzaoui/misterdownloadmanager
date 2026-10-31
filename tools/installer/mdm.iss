; Mister Download Manager's Windows installer (Inno Setup 6). Built by tools/installer/build.ps1,
; which passes /DAppVersion=... and, when a certificate is set up, /DSign with /Smdmsign=...
;
; Install for the current user (no administrator rights, %LOCALAPPDATA%\Programs) or for
; everyone (Program Files); the wizard asks. Tasks: browser integration (native messaging host
; for Chrome, Edge, Brave and Firefox), .torrent files and magnet links, start with Windows, a
; desktop shortcut. A running MDM is asked to save and quit first (/exit). Uninstalling asks
; whether to keep the download list and settings.

#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#define AppName "Mister Download Manager"
#define AppExe "misterdownloadmanager.exe"
#define HostExe "misterdownloadmanager-nmhost.exe"
#define Root "..\.."
#define Bin Root + "\target\release"

[Setup]
; Never change AppId: upgrades and the uninstaller find the installation by it.
AppId={{667DB4D8-32C2-4487-A0E5-5825BF29778C}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Ahmed Nefzaoui
AppPublisherURL=https://github.com/anefzaoui/misterdownloadmanager
AppSupportURL=https://github.com/anefzaoui/misterdownloadmanager/issues
AppUpdatesURL=https://github.com/anefzaoui/misterdownloadmanager/releases
AppCopyright=Copyright (C) 2026 Ahmed Nefzaoui
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; The Run entry is the installing user's, also when installing for everyone.
UsedUserAreasWarning=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile={#Root}\crates\app\assets\icons\app\mdm.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir={#Root}\target\dist
OutputBaseFilename=MisterDownloadManager-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
; A running MDM is asked to quit by [Code] (it holds the mutex MisterDownloadManager.Running);
; AppMutex isn't used because it would refuse to start before that. Restart Manager is the
; fallback for anything else holding the files (a browser running the host).
CloseApplications=yes
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Ahmed Nefzaoui
VersionInfoDescription={#AppName} setup
VersionInfoProductName={#AppName}
#ifdef Sign
SignTool=mdmsign
SignedUninstaller=yes
  #define SignFlag "signonce"
#else
  #define SignFlag ""
#endif

; Every interface language (i18n/languages.tsv): tools/installer/languages.py writes the list and
; the translations of the messages below. The wizard starts in Windows' display language.
#include "languages.iss"

[CustomMessages]
lang_en.TaskBrowser=Set up &browser integration (Chrome, Edge, Brave, Firefox)
lang_en.TaskTorrents=Open .&torrent files and magnet links with {#AppName}
lang_en.TaskStartup=Start {#AppName} when &Windows starts
lang_en.TaskGroupOther=Other:
lang_en.Launch=Start {#AppName}
lang_en.KeepData=Also delete your download list and settings?%n%nYour downloaded files are not touched either way.

[Tasks]
Name: "browser"; Description: "{cm:TaskBrowser}"
Name: "torrents"; Description: "{cm:TaskTorrents}"
Name: "startup"; Description: "{cm:TaskStartup}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:TaskGroupOther}"; Flags: unchecked

[Files]
Source: "{#Bin}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion {#SignFlag}
Source: "{#Bin}\{#HostExe}"; DestDir: "{app}"; Flags: ignoreversion {#SignFlag}
Source: "{#Bin}\misterdownloadmanager-cli.exe"; DestDir: "{app}"; Flags: ignoreversion {#SignFlag}
Source: "{#Bin}\misterdownloadmanager-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion {#SignFlag}
; The extension, for "Load unpacked" (Chromium) and about:debugging (Firefox) until it is in
; the stores.
Source: "{#Root}\target\extension\chromium\*"; DestDir: "{app}\extension\chromium"; Flags: ignoreversion recursesubdirs
Source: "{#Root}\target\extension\mdm-firefox.xpi"; DestDir: "{app}\extension"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#Root}\ATTRIBUTION.md"; DestDir: "{app}"; DestName: "ATTRIBUTION.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; The language chosen in the wizard ({language}: "lang_" + a languages.tsv code with "_" for "-",
; see languages.py inno_name). The app
; takes it on its first start, when it has no language setting yet (settings.rs), then deletes it.
Root: HKCU; Subkey: "Software\MisterDownloadManager"; ValueType: string; ValueName: "InstallLanguage"; ValueData: "{language}"; Flags: uninsdeletevalue uninsdeletekeyifempty
; Start with Windows: the same value Options -> "Launch on startup" writes and reads.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "MisterDownloadManager"; ValueData: """{app}\{#AppExe}"""; Flags: uninsdeletevalue; Tasks: startup
; .torrent files and magnet links (HKA: the user's or everyone's classes, as installed).
Root: HKA; Subkey: "Software\Classes\MisterDownloadManager.torrent"; ValueType: string; ValueName: ""; ValueData: "Torrent file"; Flags: uninsdeletekey; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\MisterDownloadManager.torrent\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\MisterDownloadManager.torrent\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\.torrent"; ValueType: string; ValueName: ""; ValueData: "MisterDownloadManager.torrent"; Flags: uninsdeletekeyifempty; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\.torrent\OpenWithProgids"; ValueType: string; ValueName: "MisterDownloadManager.torrent"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: ""; ValueData: "URL:Magnet link"; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\magnet"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\magnet\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: torrents
Root: HKA; Subkey: "Software\Classes\magnet\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: torrents

[Run]
Filename: "{app}\{#HostExe}"; Parameters: "--register"; Flags: runhidden; Tasks: browser; Check: not IsAdminInstallMode
Filename: "{app}\{#HostExe}"; Parameters: "--register --all-users"; Flags: runhidden; Tasks: browser; Check: IsAdminInstallMode
Filename: "{app}\{#AppExe}"; Description: "{cm:Launch}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\{#HostExe}"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "UnregisterUser"; Check: not IsAdminInstallMode
Filename: "{app}\{#HostExe}"; Parameters: "--unregister --all-users"; Flags: runhidden; RunOnceId: "UnregisterAll"; Check: IsAdminInstallMode

[UninstallDelete]
Type: files; Name: "{app}\com.misterdownloadmanager.host*.json"

[Code]
const
  RunningMutex = 'MisterDownloadManager.Running';

{ Ask a running MDM to save and quit, and wait up to 15 s for it. }
procedure ExitRunningApp(Exe: String);
var
  Code, Waited: Integer;
begin
  if CheckForMutexes(RunningMutex) and FileExists(Exe) then
  begin
    Exec(Exe, '/exit', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Waited := 0;
    while CheckForMutexes(RunningMutex) and (Waited < 15000) do
    begin
      Sleep(250);
      Waited := Waited + 250;
    end;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ExitRunningApp(ExpandConstant('{app}\{#AppExe}'));
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  ExitRunningApp(ExpandConstant('{app}\{#AppExe}'));
  Result := True;
end;

{ Remove a class key only while it still points at us (another program may have taken over
  magnet links or .torrent files since). }
procedure ForgetIfOurs(Key, Ours: String);
var
  Value: String;
begin
  if RegQueryStringValue(HKA, Key + '\shell\open\command', '', Value) and (Pos(Lowercase(Ours), Lowercase(Value)) > 0) then
    RegDeleteKeyIncludingSubkeys(HKA, Key);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value, Data: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    ForgetIfOurs('Software\Classes\magnet', ExpandConstant('{app}\{#AppExe}'));
    if RegQueryStringValue(HKA, 'Software\Classes\.torrent', '', Value) and (Value = 'MisterDownloadManager.torrent') then
      RegDeleteValue(HKA, 'Software\Classes\.torrent', '');
  end;
  if CurUninstallStep = usPostUninstall then
  begin
    Data := ExpandConstant('{userappdata}\misterdownloadmanager');
    if DirExists(Data) and not UninstallSilent() then
      if MsgBox(CustomMessage('KeepData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(Data, True, True, True);
  end;
end;
