#ifndef AppVersion
  #error Pass /DAppVersion=<Cargo package version> to ISCC.
#endif

[Setup]
AppId={{A5972086-C993-4421-8231-973FACE6C72C}
AppName=fyle
AppVersion={#AppVersion}
AppVerName=fyle {#AppVersion}
DefaultDirName={autopf}\fyle
DefaultGroupName=fyle
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
OutputDir=..\dist\windows
OutputBaseFilename=fyle-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ChangesAssociations=yes
UninstallDisplayIcon={app}\fyle.exe

[Files]
Source: "..\target\release\fyle.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\fyle"; Filename: "{app}\fyle.exe"

[Tasks]
Name: "defaultfolder"; Description: "Open folders with fyle by default"; Flags: unchecked

[Registry]
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\fyle.exe"; ValueType: string; ValueData: "{app}\fyle.exe"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\fyle.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "fyle"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\fyle.exe\shell\open\command"; ValueType: string; ValueData: """{app}\fyle.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Directory\shell\Fyle"; ValueType: string; ValueData: "Open in fyle"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\Fyle\command"; ValueType: string; ValueData: """{app}\fyle.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Drive\shell\Fyle"; ValueType: string; ValueData: "Open in fyle"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Drive\shell\Fyle\command"; ValueType: string; ValueData: """{app}\fyle.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\Fyle"; ValueType: string; ValueData: "Open in fyle"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\Fyle\command"; ValueType: string; ValueData: """{app}\fyle.exe"" ""%V"""
Root: HKA; Subkey: "Software\fyle\Installer"; Tasks: defaultfolder; Flags: uninsdeletekey

[Code]
const
  FolderShellKey = 'Software\Classes\Directory\shell';
  BackupKey = 'Software\fyle\Installer';

procedure CurStepChanged(CurStep: TSetupStep);
var
  PreviousVerb: String;
begin
  if (CurStep <> ssPostInstall) or not WizardIsTaskSelected('defaultfolder') then
    Exit;

  if not RegValueExists(HKA, BackupKey, 'HadPreviousVerb') then
  begin
    if RegQueryStringValue(HKA, FolderShellKey, '', PreviousVerb) then
    begin
      RegWriteDWordValue(HKA, BackupKey, 'HadPreviousVerb', 1);
      RegWriteStringValue(HKA, BackupKey, 'PreviousVerb', PreviousVerb);
    end
    else
      RegWriteDWordValue(HKA, BackupKey, 'HadPreviousVerb', 0);
  end;
  RegWriteStringValue(HKA, FolderShellKey, '', 'Fyle');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  CurrentVerb, PreviousVerb: String;
  HadPreviousVerb: Cardinal;
begin
  if (CurUninstallStep <> usUninstall) or
    not RegQueryStringValue(HKA, FolderShellKey, '', CurrentVerb) or
    (CurrentVerb <> 'Fyle') or
    not RegQueryDWordValue(HKA, BackupKey, 'HadPreviousVerb', HadPreviousVerb) then
    Exit;

  if HadPreviousVerb = 1 then
  begin
    if RegQueryStringValue(HKA, BackupKey, 'PreviousVerb', PreviousVerb) then
      RegWriteStringValue(HKA, FolderShellKey, '', PreviousVerb);
  end
  else
    RegDeleteValue(HKA, FolderShellKey, '');
end;
