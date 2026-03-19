!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

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

; --- Finish page: option to run the app after install ---
!define MUI_FINISHPAGE_RUN "$INSTDIR\duper-disper.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Run Duper Disper"
!define MUI_FINISHPAGE_RUN_PARAMETERS "--settings"

; --- Pages (order matters) ---
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom DesktopShortcutPage DesktopShortcutPageLeave
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; --- Variables ---
Var DesktopShortcutCheckbox
Var CreateDesktopShortcut

; --- Detect running instance and offer to terminate ---
Function .onInit
    StrCpy $CreateDesktopShortcut "1"

    ; Check if duper-disper.exe is running
    FindWindow $0 "" "" 0
    nsExec::ExecToStack 'tasklist /FI "IMAGENAME eq duper-disper.exe" /FO CSV /NH'
    Pop $0 ; exit code
    Pop $1 ; output

    ; tasklist outputs "INFO: No tasks..." when process is not found
    StrCpy $2 $1 4
    ${If} $2 != "INFO"
    ${AndIf} $1 != ""
        ; Process is running — ask user
        MessageBox MB_YESNO|MB_ICONQUESTION \
            "Duper Disper is currently running.$\n$\nDo you want to close it and continue with the installation?" \
            IDYES kill_it
        ; User chose No — abort installer
        Abort
    kill_it:
        nsExec::ExecToLog 'taskkill /F /IM duper-disper.exe'
        Sleep 1000
    ${EndIf}
FunctionEnd

; --- Custom page for desktop shortcut option ---
Function DesktopShortcutPage
    nsDialogs::Create 1018
    Pop $0
    ${If} $0 == error
        Abort
    ${EndIf}

    ${NSD_CreateLabel} 0 0 100% 24u "Choose additional options:"
    Pop $0

    ${NSD_CreateCheckbox} 12u 30u 100% 12u "Create a desktop shortcut"
    Pop $DesktopShortcutCheckbox
    ${NSD_SetState} $DesktopShortcutCheckbox ${BST_CHECKED}

    nsDialogs::Show
FunctionEnd

Function DesktopShortcutPageLeave
    ${NSD_GetState} $DesktopShortcutCheckbox $0
    ${If} $0 == ${BST_CHECKED}
        StrCpy $CreateDesktopShortcut "1"
    ${Else}
        StrCpy $CreateDesktopShortcut "0"
    ${EndIf}
FunctionEnd

; --- Installer ---
Section "Install"
    SetOutPath "$INSTDIR"

    ; Main binary
    File "duper-disper.exe"

    ; Create Start Menu shortcuts
    CreateDirectory "$SMPROGRAMS\Duper Disper"
    CreateShortCut "$SMPROGRAMS\Duper Disper\Duper Disper.lnk" "$INSTDIR\duper-disper.exe"
    CreateShortCut "$SMPROGRAMS\Duper Disper\Uninstall.lnk" "$INSTDIR\uninstall.exe"

    ; Create Desktop shortcut only if user opted in
    ${If} $CreateDesktopShortcut == "1"
        CreateShortCut "$DESKTOP\Duper Disper.lnk" "$INSTDIR\duper-disper.exe"
    ${EndIf}

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
