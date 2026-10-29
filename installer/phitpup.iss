; The PhitPup installer (Inno Setup 6.5 or later). It installs for the
; current user only, so it needs no administrator rights:
;
;   iscc installer\phitpup.iss
;   iscc /DSourceExe=path\to\phitpup.exe /DOutputDir=dist installer\phitpup.iss
;
; The version comes from the exe's own version details (see build.rs).

#ifndef SourceExe
  #define SourceExe "..\target\release\phitpup.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\target\installer"
#endif
#define AppVersion GetStringFileInfo(SourceExe, "ProductVersion")
#define Site "https://intity01.github.io/phitpup/"
#define Repo "https://github.com/intity01/phitpup"

[Setup]
; Never change AppId: it is how a new version finds the old one.
AppId={{6B0E3C1D-5F2A-4C57-9A3E-7D1F0B8C2E44}
AppName=PhitPup
AppVerName=PhitPup {#AppVersion}
AppVersion={#AppVersion}
AppPublisher=sukudone.team-dev
AppPublisherURL={#Site}
AppSupportURL={#Repo}/issues
AppUpdatesURL={#Repo}/releases
AppCopyright=© 2026 Intity01. MIT License.
VersionInfoVersion={#AppVersion}
VersionInfoProductName=PhitPup
VersionInfoDescription=PhitPup Setup
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Programs\PhitPup
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=yes
UninstallDisplayName=PhitPup
UninstallDisplayIcon={app}\phitpup.exe
SetupIconFile=..\assets\icon\phitpup.ico
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
; The running copy is asked to quit in [Code]; if it is still running,
; Setup says so instead of replacing a file in use.
AppMutex=Local\PhitPup.SingleInstance
CloseApplications=no
OutputDir={#OutputDir}
OutputBaseFilename=PhitPup-Setup
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
#ifexist AddBackslash(CompilerPath) + "Languages\Thai.isl"
Name: "th"; MessagesFile: "compiler:Languages\Thai.isl"
#endif

[CustomMessages]
en.StartWithWindows=Start PhitPup when I sign in to Windows
en.DesktopIcon=Put a shortcut on the desktop
en.Launch=Start PhitPup now
th.StartWithWindows=เปิด PhitPup เมื่อเข้าสู่ระบบ Windows
th.DesktopIcon=สร้างทางลัดบนเดสก์ท็อป
th.Launch=เปิด PhitPup เลย

[Tasks]
Name: "startup"; Description: "{cm:StartWithWindows}"
Name: "desktopicon"; Description: "{cm:DesktopIcon}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "phitpup.exe"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "..\THIRD_PARTY.md"; DestDir: "{app}"
; The fonts are inside the exe; their licence travels with them.
Source: "..\assets\fonts\OFL-*.txt"; DestDir: "{app}\licenses"

[Icons]
Name: "{userprograms}\PhitPup"; Filename: "{app}\phitpup.exe"; Comment: "Fix text typed in the wrong keyboard layout"
Name: "{userdesktop}\PhitPup"; Filename: "{app}\phitpup.exe"; Tasks: desktopicon

[Registry]
; The same value the app's own "Start with Windows" switch writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "PhitPup"; ValueData: """{app}\phitpup.exe"""; Tasks: startup

[Run]
Filename: "{app}\phitpup.exe"; Description: "{cm:Launch}"; Flags: nowait postinstall skipifsilent

[Code]
const
  // WM_APP + 12, WM_QUIT_REQUEST in crates/phitpup/src/tray.rs.
  WM_QUIT_REQUEST = $800C;
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

// Asks a running PhitPup to quit and waits a few seconds for it.
procedure CloseRunningCopy;
var
  Wnd: HWND;
  Tries: Integer;
begin
  Wnd := FindWindowByClassName('PhitPupTray');
  if Wnd = 0 then
    Exit;
  PostMessage(Wnd, WM_QUIT_REQUEST, 0, 0);
  Tries := 0;
  while CheckForMutexes('Local\PhitPup.SingleInstance') and (Tries < 30) do
  begin
    Sleep(100);
    Tries := Tries + 1;
  end;
end;

function InitializeSetup: Boolean;
begin
  CloseRunningCopy;
  Result := True;
end;

function InitializeUninstall: Boolean;
begin
  CloseRunningCopy;
  Result := True;
end;

// Removes "start with Windows" if it points at this copy, whether Setup or
// the app's own switch wrote it. Settings in %APPDATA%\PhitPup are kept.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep <> usUninstall then
    Exit;
  if RegQueryStringValue(HKCU, RunKey, 'PhitPup', Data) and
     (Pos(Lowercase(ExpandConstant('{app}')), Lowercase(Data)) > 0) then
    RegDeleteValue(HKCU, RunKey, 'PhitPup');
end;
