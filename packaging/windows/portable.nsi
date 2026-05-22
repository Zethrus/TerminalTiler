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

Name "TerminalTiler Portable"
OutFile "${OUT_FILE}"

Section "Run"
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /r "${STAGE_DIR}\*"

  ClearErrors
  ExecWait '"$PLUGINSDIR\TerminalTiler.exe"' $0
  IfErrors 0 +2
    StrCpy $0 1

  SetOutPath "$TEMP"
  RMDir /r "$PLUGINSDIR"
  SetErrorLevel $0
SectionEnd
