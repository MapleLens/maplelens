; MapleLens installer: maplelens-vX.Y.Z-setup.exe, built by release.yml next to the zip. Inno Setup 7 (6.6 and later
; compile it too: windows-latest has 6.7 preinstalled).
;
;   cargo build --release -p maplelens
;   ./scripts/build-installer.ps1          (or: ISCC /DAppVersion=X.Y.Z installer\maplelens.iss, into target\installer)
;
; Per user, without administrator: the self-updater (crates/win/src/update/install.rs) swaps maplelens.exe in place
; and the log is written next to it (log.rs), so the folder must be the player's to write. The zip and its signed
; manifest stay what the updater downloads; this only puts the same exe in a folder with shortcuts and an uninstaller.

#if Ver < EncodeVer(6, 6, 0)
  #error Inno Setup 6.6 or later is needed (WizardStyle "dynamic")
#endif
#ifndef AppVersion
  #error Pass the version: ISCC /DAppVersion=X.Y.Z installer\maplelens.iss
#endif
#ifndef ExeSource
  #define ExeSource "..\target\release\maplelens.exe"
#endif
; Windows' file version takes numbers only: a pre-release's "-rc1" is left out there.
#if Pos("-", AppVersion) > 0
  #define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion) - 1)
#else
  #define NumericVersion AppVersion
#endif

[Setup]
; Never change: a new installer finds the old install by it and upgrades in place. The app keeps the uninstall
; entry's version current after a self-update by the same id (crates/win/src/installed.rs).
AppId={{702546F1-91F5-4C7A-BED5-11F8C72791DC}
AppName=MapleLens
AppVersion={#AppVersion}
AppVerName=MapleLens {#AppVersion}
AppPublisher=MapleLens contributors
AppPublisherURL=https://maple.observer/
AppSupportURL=https://github.com/MapleLens/maplelens/issues
AppUpdatesURL=https://github.com/MapleLens/maplelens/releases
AppCopyright=Copyright (c) 2026 MapleLens contributors
VersionInfoVersion={#NumericVersion}
VersionInfoProductName=MapleLens
VersionInfoDescription=MapleLens Setup
PrivilegesRequired=lowest
; Under lowest privileges this is %LOCALAPPDATA%\Programs\MapleLens.
DefaultDirName={autopf}\MapleLens
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; Light or dark as Windows is.
WizardStyle=modern dynamic
SetupIconFile=..\crates\win\assets\maplelens.ico
UninstallDisplayIcon={app}\maplelens.exe
UninstallDisplayName=MapleLens
OutputDir=..\target\installer
OutputBaseFilename=maplelens-v{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes
SetupMutex=MapleLens.Setup
; A running copy is asked to be closed by the player (see [Code]): the Restart Manager cannot close one that runs as
; administrator, and the tray's Exit is how MapleLens is meant to stop.
CloseApplications=no
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; A self-update interrupted half way (update/install.rs swap).
Type: files; Name: "{app}\maplelens.old.exe"
Type: files; Name: "{app}\maplelens.new.exe"

[Files]
; The name matters: the updater only replaces an exe called maplelens.exe (update::install::can_self_update).
Source: "{#ExeSource}"; DestDir: "{app}"; DestName: "maplelens.exe"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\MapleLens"; Filename: "{app}\maplelens.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\MapleLens"; Filename: "{app}\maplelens.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
; Started as the player, not elevated by Setup: the app asks for administrator itself (elevate.rs), and that prompt
; can be declined into a working app.
Filename: "{app}\maplelens.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,MapleLens}"; Flags: nowait postinstall skipifsilent shellexec

[UninstallDelete]
; Files the app makes next to itself: the updater's leftovers and the log with its rotations (log.rs).
Type: files; Name: "{app}\maplelens.old.exe"
Type: files; Name: "{app}\maplelens.new.exe"
Type: files; Name: "{app}\maplelens.log"
Type: files; Name: "{app}\maplelens.log.*"

[Code]
const
  // main.rs `already_running`: every running copy holds this mutex.
  RunningMutex = 'Local\MapleLens.SingleInstance';
  // startup.rs: "Start with Windows".
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  RunValue = 'MapleLens';
  ERROR_ACCESS_DENIED = 5;
  ERROR_ALREADY_EXISTS = 183;

function CreateMutex(Attributes: Longint; InitialOwner: Longint; Name: String): THandle;
  external 'CreateMutexW@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Longint;
  external 'CloseHandle@kernel32.dll stdcall';

// AppMutex would open the mutex, and that fails with access denied while MapleLens runs as administrator (it asks
// to, elevate.rs) and Setup does not, which reads as not running. Creating it tells both apart from not running.
function MapleLensRunning: Boolean;
var
  Mutex: THandle;
  Error: Longint;
begin
  Mutex := CreateMutex(0, 0, RunningMutex);
  Error := DLLGetLastError;
  if Mutex <> 0 then
    CloseHandle(Mutex);
  Result := (Error = ERROR_ALREADY_EXISTS) or ((Mutex = 0) and (Error = ERROR_ACCESS_DENIED));
end;

// A running exe is never replaced or removed: the player exits it, or Setup stops (always, when silent).
function WaitForExit: Boolean;
begin
  Result := True;
  while MapleLensRunning do
    if SuppressibleMsgBox('MapleLens is running. Exit it first: right-click the meso coin in the tray (bottom right ' +
        'of the screen) and choose Exit. Then click Retry.', mbError, MB_RETRYCANCEL, IDCANCEL) = IDCANCEL then
    begin
      Result := False;
      Exit;
    end;
end;

function InitializeSetup: Boolean;
begin
  Result := WaitForExit;
end;

// Again just before the files: it may have been started while the wizard was open.
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  if WaitForExit then
    Result := ''
  else
    Result := 'MapleLens is still running. Exit it from its tray icon and run Setup again.';
end;

function InitializeUninstall: Boolean;
begin
  Result := WaitForExit;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command, Data: String;
begin
  // Only an entry that starts this copy: a portable copy elsewhere keeps its own.
  if (CurUninstallStep = usUninstall) and RegQueryStringValue(HKCU, RunKey, RunValue, Command) and
      (Pos(Lowercase(ExpandConstant('{app}\maplelens.exe')), Lowercase(Command)) > 0) then
    RegDeleteValue(HKCU, RunKey, RunValue);
  // The player's own data (settings, training history, the store book, caches) stays unless they ask.
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
  begin
    Data := ExpandConstant('{localappdata}\MapleLens');
    if DirExists(Data) and (MsgBox('Also delete your MapleLens settings, training history, store book and ' +
        'downloaded data?' + #13#10#13#10 + Data, mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
      DelTree(Data, True, True, True);
  end;
end;
