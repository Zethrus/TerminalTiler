Unicode true
RequestExecutionLevel user

!ifndef APP_VERSION
  !error "APP_VERSION define is required"
!endif

!ifndef STAGE_DIR
  !error "STAGE_DIR define is required"
!endif

!ifndef OUT_FILE
  !error "OUT_FILE define is required"
!endif

Name "TerminalTiler"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\TerminalTiler"
InstallDirRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "InstallLocation"
ShowInstDetails show
ShowUnInstDetails show

Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Section "Install"
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${STAGE_DIR}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\TerminalTiler"
  CreateShortcut "$SMPROGRAMS\TerminalTiler\TerminalTiler.lnk" "$INSTDIR\TerminalTiler.exe"
  CreateShortcut "$SMPROGRAMS\TerminalTiler\Uninstall TerminalTiler.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "DisplayName" "TerminalTiler"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "NoRepair" 1

  IfSilent +2 0
  MessageBox MB_OK "Browser tiles require Microsoft Edge WebView2 Runtime. Install the Evergreen runtime from https://go.microsoft.com/fwlink/p/?LinkId=2124703 before opening presets or restored sessions that include web tiles."
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\TerminalTiler\TerminalTiler.lnk"
  Delete "$SMPROGRAMS\TerminalTiler\Uninstall TerminalTiler.lnk"
  RMDir "$SMPROGRAMS\TerminalTiler"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler"
SectionEnd
