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

  nsExec::ExecToLog 'powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$INSTDIR\lib\mymedia\installer\windows-task.ps1" -Action install -InstallDirectory "$INSTDIR"'
  Pop $0
  ${If} $0 != 0
    MessageBox MB_ICONSTOP "ZMydia was installed, but its background service could not be registered (exit code $0)."
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
SectionEnd
