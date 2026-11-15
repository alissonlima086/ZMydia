Unicode True
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef PACKAGE_DIR
  !error "PACKAGE_DIR is required"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE is required"
!endif
!ifndef PRODUCT_VERSION
  !error "PRODUCT_VERSION is required"
!endif

Name "ZMydia ${PRODUCT_VERSION}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\ZMydia"
InstallDirRegKey HKCU "Software\ZMydia" "InstallDirectory"
BrandingText "ZMydia"
SetCompressor /SOLID lzma

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$WINDIR\explorer.exe"
!define MUI_FINISHPAGE_RUN_PARAMETERS "http://127.0.0.1:4173"
!define MUI_FINISHPAGE_RUN_TEXT "Open ZMydia"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "PortugueseBR"

Section "Install"
  SetShellVarContext current
  StrCpy $1 "0"

  ; An existing scheduled task must release the executable before files are replaced.
  IfFileExists "$INSTDIR\lib\mymedia\installer\windows-task.ps1" 0 pre_copy
  nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action uninstall -InstallDirectory "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP|MB_OK "The running ZMydia service could not be stopped (exit code $0)." /SD IDOK
    Abort
  ${EndIf}
  RMDir /r "$INSTDIR.previous"
  ClearErrors
  Rename "$INSTDIR" "$INSTDIR.previous"
  IfErrors backup_failed backup_ready
  backup_failed:
    MessageBox MB_ICONSTOP|MB_OK "The previous ZMydia installation could not be preserved for rollback." /SD IDOK
    Abort
  backup_ready:
  StrCpy $1 "1"

  pre_copy:
  SetOutPath "$INSTDIR"
  File /r "${PACKAGE_DIR}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\ZMydia" "InstallDirectory" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "DisplayName" "ZMydia"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "DisplayVersion" "${PRODUCT_VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "Publisher" "ZMydia"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia" "NoRepair" 1

  CreateDirectory "$SMPROGRAMS\ZMydia"
  CreateShortcut "$SMPROGRAMS\ZMydia\Open ZMydia.lnk" "$WINDIR\explorer.exe" "http://127.0.0.1:4173"
  CreateShortcut "$SMPROGRAMS\ZMydia\Uninstall ZMydia.lnk" "$INSTDIR\Uninstall.exe"

  nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action install -InstallDirectory "$INSTDIR" -ExpectedVersion "${PRODUCT_VERSION}"'
  Pop $0
  ${If} $0 != 0
    ${If} $1 == "1"
      nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action uninstall -InstallDirectory "$INSTDIR"'
      Pop $2
      RMDir /r "$INSTDIR"
      Rename "$INSTDIR.previous" "$INSTDIR"
      nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action install -InstallDirectory "$INSTDIR"'
      Pop $2
    ${EndIf}
    MessageBox MB_ICONSTOP|MB_OK "ZMydia was installed, but its background service could not be registered (exit code $0)." /SD IDOK
    Abort
  ${EndIf}
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action uninstall -InstallDirectory "$INSTDIR"'
  Pop $0

  Delete "$SMPROGRAMS\ZMydia\Open ZMydia.lnk"
  Delete "$SMPROGRAMS\ZMydia\Uninstall ZMydia.lnk"
  RMDir "$SMPROGRAMS\ZMydia"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZMydia"
  DeleteRegKey HKCU "Software\ZMydia"
  RMDir /r "$INSTDIR"
  RMDir /r "$INSTDIR.previous"
SectionEnd
