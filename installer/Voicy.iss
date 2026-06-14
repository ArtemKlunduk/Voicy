; ── Voicy installer (Inno Setup 6) ─────────────────────────────────────────
; Telegram voice-messaging app. Per-user install (no admin / UAC) to
; %LOCALAPPDATA%\Programs\Voicy — must be writable because the app stores its
; config (voicy.toml) next to the exe. ASR models + Telegram session live in
; %APPDATA%\voicy and survive reinstalls/uninstalls.

#define MyAppName "Voicy"
; ВНИМАНИЕ: держать в синхроне с [package] version в ../Cargo.toml (сейчас 0.1.0).
#define MyAppVersion "0.1.0"
; Издатель — поставь сюда своё имя/ник перед публичной раздачей.
#define MyAppPublisher "Voicy"
#define MyAppURL "https://github.com/ArtemKlunduk/Voicy"
#define MyAppExe "voicy.exe"

; PUBLICBUILD=1 → shareable build (ships voicy.toml.example, no credentials).
; PUBLICBUILD=0 (default) → personal build (ships voicy.toml with API credentials).
; Override from the command line: ISCC /DPUBLICBUILD=1 Voicy.iss
#ifndef PUBLICBUILD
  #define PUBLICBUILD 0
#endif

[Setup]
AppId={{8F2A1C7E-4B3D-4A9E-9C5F-1D2E3F4A5B6C}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile=payload\LICENSE.txt
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
#if Int(PUBLICBUILD) == 1
OutputBaseFilename=voicy-setup-{#MyAppVersion}-public
#else
OutputBaseFilename=voicy-setup-{#MyAppVersion}
#endif
SetupIconFile=payload\voicy.ico
UninstallDisplayIcon={app}\voicy.ico
UninstallDisplayName={#MyAppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "startupicon"; Description: "Run Voicy when Windows starts"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "payload\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\onnxruntime.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\WebView2Loader.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\msvcp140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\msvcp140_1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\vcruntime140.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\vcruntime140_1.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
; App icon — used for shortcuts and the Add/Remove Programs entry.
Source: "payload\voicy.ico"; DestDir: "{app}"; Flags: ignoreversion
#if Int(PUBLICBUILD) == 1
; Public build: credentials template + readme; user creates their own voicy.toml.
Source: "payload\voicy.toml.example"; DestDir: "{app}"; Flags: ignoreversion
Source: "payload\README.txt"; DestDir: "{app}"; Flags: ignoreversion
#else
; Personal build: config with API credentials — keep user's existing one on reinstall.
Source: "payload\voicy.toml"; DestDir: "{app}"; Flags: onlyifdoesntexist uninsneveruninstall
#endif
; WebView2 evergreen bootstrapper — used during setup only, not installed.
Source: "payload\MicrosoftEdgeWebview2Setup.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\voicy.ico"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\voicy.ico"; Tasks: desktopicon

[Run]
; Install WebView2 Runtime first if the system doesn't have it.
Filename: "{tmp}\MicrosoftEdgeWebview2Setup.exe"; Parameters: "/silent /install"; StatusMsg: "Installing Microsoft WebView2 Runtime..."; Check: WebView2Missing; Flags: waituntilterminated
; Offer to launch after install.
Filename: "{app}\{#MyAppExe}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent

[Registry]
; Optional autostart (only if the startupicon task is selected).
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Voicy"; ValueData: """{app}\{#MyAppExe}"""; Flags: uninsdeletevalue; Tasks: startupicon

[Code]
function WebView2Installed(): Boolean;
var
  pv: String;
begin
  Result := False;
  // Machine-wide (64-bit) install of the Evergreen runtime.
  if RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', pv) then
    if (pv <> '') and (pv <> '0.0.0.0') then
      Result := True;
  // Per-user install.
  if not Result then
    if RegQueryStringValue(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', pv) then
      if (pv <> '') and (pv <> '0.0.0.0') then
        Result := True;
end;

function WebView2Missing(): Boolean;
begin
  Result := not WebView2Installed();
end;
