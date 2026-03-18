!include "MUI2.nsh"

; --- General ---
Name "Duper Disper"
OutFile "duper-disper-setup.exe"
InstallDir "$PROGRAMFILES64\Duper Disper"
InstallDirRegKey HKLM "Software\DuperDisper" "InstallDir"
RequestExecutionLevel admin

; --- Interface ---
!define MUI_ABORTWARNING
!define MUI_ICON "icon.ico"
!define MUI_UNICON "icon.ico"

; --- Pages ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Installer ---
Section "Install"
    SetOutPath "$INSTDIR"

    ; Main binary
    File "duper-disper.exe"

    ; Create Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\Duper Disper"
    CreateShortCut "$SMPROGRAMS\Duper Disper\Duper Disper.lnk" "$INSTDIR\duper-disper.exe"
    CreateShortCut "$SMPROGRAMS\Duper Disper\Uninstall.lnk" "$INSTDIR\uninstall.exe"

    ; Create Desktop shortcut
    CreateShortCut "$DESKTOP\Duper Disper.lnk" "$INSTDIR\duper-disper.exe"

    ; Auto-start with Windows (optional, via registry)
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DuperDisper" "$INSTDIR\duper-disper.exe"

    ; Write registry keys for uninstaller
    WriteRegStr HKLM "Software\DuperDisper" "InstallDir" "$INSTDIR"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "DisplayName" "Duper Disper"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "UninstallString" "$INSTDIR\uninstall.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "DisplayIcon" "$INSTDIR\duper-disper.exe"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "Publisher" "Duper Disper"
    WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "DisplayVersion" "${VERSION}"
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "NoModify" 1
    WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper" "NoRepair" 1

    ; Write uninstaller
    WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

; --- Uninstaller ---
Section "Uninstall"
    ; Kill running instance
    nsExec::ExecToLog 'taskkill /F /IM duper-disper.exe'

    ; Remove files
    Delete "$INSTDIR\duper-disper.exe"
    Delete "$INSTDIR\uninstall.exe"

    ; Remove shortcuts
    Delete "$SMPROGRAMS\Duper Disper\Duper Disper.lnk"
    Delete "$SMPROGRAMS\Duper Disper\Uninstall.lnk"
    RMDir "$SMPROGRAMS\Duper Disper"
    Delete "$DESKTOP\Duper Disper.lnk"

    ; Remove auto-start
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DuperDisper"

    ; Remove registry keys
    DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\DuperDisper"
    DeleteRegKey HKLM "Software\DuperDisper"

    ; Remove install directory
    RMDir "$INSTDIR"
SectionEnd
