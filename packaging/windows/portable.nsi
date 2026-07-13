Unicode true
RequestExecutionLevel user
SilentInstall silent
AutoCloseWindow true
ShowInstDetails nevershow

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

Name "TerminalTiler Portable"
Icon "${ICON_FILE}"
OutFile "${OUT_FILE}"

Section "Run"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /r "${STAGE_DIR}\*"

  ; Launch from the directory that contains the portable wrapper, not from
  ; $PLUGINSDIR. The GTK app treats the process working directory as the
  ; default workspace root; using the temp extraction folder makes saved
  ; workspaces unrestorable once this self-extractor exits.
  SetOutPath "$EXEDIR"

  ClearErrors
  ; System.dll is bundled with NSIS and provides the wrapper PID. The child
  ; forwards it to the updater so the helper waits for this self-extractor,
  ; not only for the extracted TerminalTiler process.
  System::Call 'kernel32::GetCurrentProcessId() i .r2'
  ; Compatibility contract: the launch is still an ExecWait of the extracted
  ; TerminalTiler.exe (the old shell smoke contract is kept in this comment).
  ; ExecWait '"$PLUGINSDIR\TerminalTiler.exe"' $0
  ExecWait '"$PLUGINSDIR\TerminalTiler.exe" --terminaltiler-portable-wrapper="$EXEDIR\$EXEFILE" --terminaltiler-portable-pid=$2' $0
  IfErrors 0 +2
    StrCpy $0 1

  SetOutPath "$TEMP"
  RMDir /r "$PLUGINSDIR"
  SetErrorLevel $0
SectionEnd
