; FSearch (Rust) installer: makensis -DVERSION=x.y.z -DSRC=<dir with fsearch.exe> -DOUT=<setup.exe> fsearch.nsi
Unicode true
SetCompressor /SOLID lzma
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
Name "FSearch ${VERSION}"
OutFile "${OUT}"
InstallDir "$PROGRAMFILES64\FSearch"
InstallDirRegKey HKLM "Software\FSearch" "InstallDir"
RequestExecutionLevel admin
BrandingText "FSearch ${VERSION}"

!define MUI_ICON "${SRC}\fsearch.ico"
!define MUI_UNICON "${SRC}\fsearch.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\fsearch.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Start FSearch"
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${SRC}\LICENSE.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "FSearch"
VIAddVersionKey "FileDescription" "FSearch installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GPL-2.0-or-later"

Section "FSearch" SecMain
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${SRC}\fsearch.exe"
  File "${SRC}\fsearch.ico"
  File "${SRC}\LICENSE.txt"
  File "${SRC}\README.txt"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateDirectory "$SMPROGRAMS\FSearch"
  CreateShortcut "$SMPROGRAMS\FSearch\FSearch.lnk" "$INSTDIR\fsearch.exe" "" "$INSTDIR\fsearch.ico"
  CreateShortcut "$SMPROGRAMS\FSearch\Uninstall FSearch.lnk" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\FSearch" "InstallDir" "$INSTDIR"
  !define UNINST "Software\Microsoft\Windows\CurrentVersion\Uninstall\FSearch"
  WriteRegStr HKLM "${UNINST}" "DisplayName" "FSearch"
  WriteRegStr HKLM "${UNINST}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKLM "${UNINST}" "Publisher" "FSearch contributors"
  WriteRegStr HKLM "${UNINST}" "DisplayIcon" "$INSTDIR\fsearch.ico"
  WriteRegStr HKLM "${UNINST}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKLM "${UNINST}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKLM "${UNINST}" "NoModify" 1
  WriteRegDWORD HKLM "${UNINST}" "NoRepair" 1
SectionEnd

Section "Desktop shortcut" SecDesktop
  CreateShortcut "$DESKTOP\FSearch.lnk" "$INSTDIR\fsearch.exe" "" "$INSTDIR\fsearch.ico"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\fsearch.exe"
  Delete "$INSTDIR\fsearch.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\FSearch\FSearch.lnk"
  Delete "$SMPROGRAMS\FSearch\Uninstall FSearch.lnk"
  RMDir "$SMPROGRAMS\FSearch"
  Delete "$DESKTOP\FSearch.lnk"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\FSearch"
  DeleteRegKey HKLM "Software\FSearch"
  ; settings (%APPDATA%\fsearch) and the index (%LOCALAPPDATA%\fsearch) are kept
SectionEnd
