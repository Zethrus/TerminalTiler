Unicode true
RequestExecutionLevel user

!define WEBVIEW2_CLIENT_GUID "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
!define WEBVIEW2_DOWNLOAD_URL "https://go.microsoft.com/fwlink/p/?LinkId=2124703"

!ifndef APP_VERSION
  !error "APP_VERSION define is required"
!endif

!ifndef STAGE_DIR
  !error "STAGE_DIR define is required"
!endif

!ifndef OUT_FILE
  !error "OUT_FILE define is required"
!endif

!ifndef ICON_FILE
  !error "ICON_FILE define is required"
!endif

!ifndef WEBVIEW2_BOOTSTRAPPER
  !error "WEBVIEW2_BOOTSTRAPPER define is required"
!endif

Name "TerminalTiler"
Icon "${ICON_FILE}"
UninstallIcon "${ICON_FILE}"
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
  ; The staged payload is also used for portable builds. Mark this copy as an
  ; installed NSIS build so the Core updater never treats a ZIP as updateable.
  FileOpen $0 "$INSTDIR\terminaltiler-install-kind" w
  FileWrite $0 "nsis"
  FileClose $0
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\TerminalTiler"
  CreateShortcut "$SMPROGRAMS\TerminalTiler\TerminalTiler.lnk" "$INSTDIR\TerminalTiler.exe" "" "$INSTDIR\share\terminaltiler.ico"
  CreateShortcut "$SMPROGRAMS\TerminalTiler\Uninstall TerminalTiler.lnk" "$INSTDIR\Uninstall.exe" "" "$INSTDIR\share\terminaltiler.ico"

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "DisplayName" "TerminalTiler"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "Publisher" "Zethrus"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "DisplayVersion" "${APP_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "DisplayIcon" "$INSTDIR\share\terminaltiler.ico"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Zethrus\TerminalTiler" "InstallerKind" "nsis"
  WriteRegStr HKCU "Software\Zethrus\TerminalTiler" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "UninstallString" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "URLInfoAbout" "https://terminaltiler.app"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "HelpLink" "https://terminaltiler.app"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler" "NoRepair" 1

  Call EnsureWebView2Runtime
SectionEnd

Function MarkWebView2RuntimeIfVersionPresent
  StrCmp $1 "" done
  StrCmp $1 "0.0.0.0" done
  StrCpy $0 "1"

  done:
FunctionEnd

Function DetectWebView2Runtime
  StrCpy $0 "0"

  ; Microsoft documents the WebView2 runtime pv value under the EdgeUpdate
  ; Clients key. A 32-bit NSIS process reads the WOW6432Node HKLM view on
  ; 64-bit Windows when SetRegView 32 is active. HKCU covers per-user installs.
  SetRegView 32
  ReadRegStr $1 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_CLIENT_GUID}" "pv"
  Call MarkWebView2RuntimeIfVersionPresent
  ReadRegStr $1 HKCU "Software\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_CLIENT_GUID}" "pv"
  Call MarkWebView2RuntimeIfVersionPresent

  ; Also inspect the native HKLM view so the check works with either runtime
  ; registration shape on current and future Windows 11 systems.
  SetRegView 64
  ReadRegStr $1 HKLM "SOFTWARE\Microsoft\EdgeUpdate\Clients\${WEBVIEW2_CLIENT_GUID}" "pv"
  Call MarkWebView2RuntimeIfVersionPresent
  SetRegView 32
FunctionEnd

Function ReportWebView2InstallFailure
  DetailPrint "Microsoft Edge WebView2 Runtime automatic installation failed: $2"
  IfSilent silent interactive

  silent:
    SetErrorLevel 2
    Goto done

  interactive:
    MessageBox MB_ICONEXCLAMATION|MB_OK "TerminalTiler installed, but Microsoft Edge WebView2 Runtime could not be installed automatically.$\r$\n$\r$\nBrowser tiles will be unavailable until you install the Evergreen runtime from ${WEBVIEW2_DOWNLOAD_URL}.$\r$\n$\r$\nDetails: $2"

  done:
FunctionEnd

Function EnsureWebView2Runtime
  Call DetectWebView2Runtime
  StrCmp $0 "1" runtime_present

  DetailPrint "Microsoft Edge WebView2 Runtime not detected; installing Evergreen runtime..."
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=MicrosoftEdgeWebview2Setup.exe "${WEBVIEW2_BOOTSTRAPPER}"

  ClearErrors
  ExecWait '"$PLUGINSDIR\MicrosoftEdgeWebview2Setup.exe" /silent /install' $1
  IfErrors exec_error
  StrCmp $1 "0" 0 exit_error

  Sleep 2000
  Call DetectWebView2Runtime
  StrCmp $0 "1" install_complete post_check_error

  exec_error:
    StrCpy $2 "could not launch the bundled WebView2 Evergreen bootstrapper"
    Call ReportWebView2InstallFailure
    Goto done

  exit_error:
    StrCpy $2 "WebView2 Evergreen bootstrapper exited with code $1"
    Call ReportWebView2InstallFailure
    Goto done

  post_check_error:
    StrCpy $2 "WebView2 Runtime was still not detected after the bootstrapper completed"
    Call ReportWebView2InstallFailure
    Goto done

  install_complete:
    DetailPrint "Microsoft Edge WebView2 Runtime installed."
    Goto done

  runtime_present:
    DetailPrint "Microsoft Edge WebView2 Runtime already installed."

  done:
FunctionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\TerminalTiler\TerminalTiler.lnk"
  Delete "$SMPROGRAMS\TerminalTiler\Uninstall TerminalTiler.lnk"
  RMDir "$SMPROGRAMS\TerminalTiler"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\TerminalTiler"
  DeleteRegKey HKCU "Software\Zethrus\TerminalTiler"
SectionEnd
