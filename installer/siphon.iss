; Siphon Downloader — Windows Standard Installer Script (Inno Setup 6)
#define MyAppName "Siphon Downloader"
#define MyAppVersion "0.2.0"
#define MyAppPublisher "COMICFLY"
#define MyAppURL "https://github.com/COMICFLY/siphon-downloader"
#define MyAppExeName "siphon.exe"

[Setup]
AppId={{8D6E4B92-3F1C-4A8D-9C65-7F4A1E92D301}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\Siphon
DefaultGroupName=Siphon Downloader
AllowNoIcons=yes
LicenseFile=..\LICENSE
OutputDir=..\dist
OutputBaseFilename=Siphon-Setup-{#MyAppVersion}-x64
SetupIconFile=..\assets\siphon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
CloseApplications=yes
ChangesEnvironment=no

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加任务："
Name: "autostart"; Description: "开机静默启动（登录 Windows 后自动常驻托盘）"; GroupDescription: "附加任务："

[Files]
Source: "..\target\release\siphon.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\siphon-native-host.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\browser-extension\*"; DestDir: "{app}\browser-extension"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\dist\siphon-browser-extension-v{#MyAppVersion}.zip"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Siphon Downloader"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"
Name: "{group}\浏览器扩展目录 (browser-extension)"; Filename: "{app}\browser-extension"
Name: "{group}\卸载 Siphon Downloader"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Siphon Downloader"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
; Optional autostart entry (launches silently into system tray via --minimized)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Siphon"; ValueData: """{app}\{#MyAppExeName}"" --minimized"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "立即启动 Siphon Downloader"; Flags: nowait postinstall skipifsilent

[Code]
var
  DataDirPage: TInputDirWizardPage;

procedure InitializeWizard;
begin
  DataDirPage := CreateInputDirPage(
    wpSelectDir,
    '选择数据存储目录',
    '指定 Siphon 保存配置、下载记录数据库与外部插件的目录',
    'Siphon 会将 config.toml、siphon.db、plugins 与浏览器扩展配置保存在以下目录。' + #13#10 +
    '您可以保持默认，或选择任意磁盘目录实现完全可控的数据存储（安装后也可随时在软件设置中修改）。',
    False,
    'SiphonData'
  );
  DataDirPage.Add('数据目录路径：');
  DataDirPage.Values[0] := ExpandConstant('{userappdata}\Siphon');
end;

function GetDataDir(Param: String): String;
begin
  Result := DataDirPage.Values[0];
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  DataDir: String;
  PointerFile: String;
begin
  if CurStep = ssPostInstall then
  begin
    DataDir := Trim(DataDirPage.Values[0]);
    if DataDir <> '' then
    begin
      ForceDirectories(DataDir);
      PointerFile := ExpandConstant('{app}\data_dir.txt');
      SaveStringToFile(PointerFile, Utf8Encode(DataDir), False);
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    // Ensure autostart key is removed even if toggled inside the app after installation
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Siphon');
  end;
end;
