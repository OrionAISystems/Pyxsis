; Capture storage is independent of the application installation directory.
!include nsDialogs.nsh
!include LogicLib.nsh
Var PyxsisCaptureRoot
Var PyxsisCaptureInput
Var PyxsisCaptureDialog
Var PyxsisInstallLocationChecked

; Older installer QA could leave its TEMP location registered. Do not offer
; that development fixture as a production default. Explicit /D locations and
; ordinary user-selected install folders remain unchanged.
Function PyxsisCheckInstallLocation
  ${If} $PyxsisInstallLocationChecked == "1"
    Return
  ${EndIf}
  StrCpy $PyxsisInstallLocationChecked "1"
  ; NSIS removes its own /D argument from $CMDLINE; inspect the original
  ; Windows command line so an explicit TEMP test directory remains valid.
  System::Call 'kernel32::GetCommandLineW() t.r0'
  ClearErrors
  ${GetOptions} $0 "/D=" $1
  ${IfNot} ${Errors}
    Return
  ${EndIf}
  StrCpy $0 "$TEMP\pyxsis-installer-qa-"
  StrLen $1 $0
  StrCpy $2 $INSTDIR $1
  ${If} $2 == $0
    StrCpy $INSTDIR "$LOCALAPPDATA\Pyxsis"
  ${EndIf}
FunctionEnd

; Registered before the stock Tauri pages: a small, explicit storage-choice page.
Page custom PyxsisCapturesPage PyxsisCapturesLeave

Function PyxsisCapturesPage
  Call PyxsisCheckInstallLocation
  ${GetOptions} $CMDLINE "/P" $0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}
  ${GetOptions} $CMDLINE "/UPDATE" $0
  ${IfNot} ${Errors}
    Abort
  ${EndIf}
  ReadRegStr $PyxsisCaptureRoot HKCU "Software\OrionAISystems\Pyxsis" "CaptureRoot"
  ${If} $PyxsisCaptureRoot == ""
    ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders" "My Pictures"
    ${If} $0 == ""
      StrCpy $0 "$PROFILE\Pictures"
    ${EndIf}
    ExpandEnvStrings $0 $0
    StrCpy $PyxsisCaptureRoot "$0\Pyxsis"
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Where should Pyxsis save captures?" "Choose your capture folder. You can change it later in Settings."
  nsDialogs::Create 1018
  Pop $PyxsisCaptureDialog
  ${If} $PyxsisCaptureDialog == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 8u 100% 30u "By default, screenshots and recordings stay in your Pictures folder, inside Pyxsis."
  Pop $0
  ${NSD_CreateDirRequest} 0 45u 78% 14u "$PyxsisCaptureRoot"
  Pop $PyxsisCaptureInput
  ${NSD_CreateBrowseButton} 80% 45u 20% 14u "Browse..."
  Pop $0
  ${NSD_OnClick} $0 PyxsisBrowseCaptures
  ${NSD_CreateLabel} 0 76u 100% 45u "Pyxsis creates two subfolders:$\r$\nPictures — PNG screenshots$\r$\nRecordings — MP4 videos"
  Pop $0
  nsDialogs::Show
FunctionEnd

Function PyxsisBrowseCaptures
  Pop $0
  ${NSD_GetText} $PyxsisCaptureInput $0
  nsDialogs::SelectFolderDialog "Choose a Pyxsis capture folder" "$0"
  Pop $0
  ${If} $0 != error
    ${NSD_SetText} $PyxsisCaptureInput $0
  ${EndIf}
FunctionEnd

Function PyxsisCapturesLeave
  ${NSD_GetText} $PyxsisCaptureInput $PyxsisCaptureRoot
  ${If} $PyxsisCaptureRoot == ""
    MessageBox MB_ICONEXCLAMATION "Choose a capture folder."
    Abort
  ${EndIf}
  ; Reject relative paths while keeping local drive and UNC screenshot locations valid.
  StrCpy $0 $PyxsisCaptureRoot 1 1
  StrCpy $1 $PyxsisCaptureRoot 2
  ${If} $0 != ":"
  ${AndIf} $1 != "\\"
    MessageBox MB_ICONEXCLAMATION "Choose an absolute folder path."
    Abort
  ${EndIf}
FunctionEnd

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack '"$INSTDIR\pyxsis-cli.exe" installer-path'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "Could not register the CLI on user PATH: $1"
    MessageBox MB_ICONEXCLAMATION "Pyxsis was installed, but could not add the CLI to your user PATH. Add the installation folder manually to use pyxsis-cli from a terminal." /SD IDOK
  ${EndIf}
  ${If} $PyxsisCaptureRoot != ""
    WriteRegStr HKCU "Software\OrionAISystems\Pyxsis" "CaptureRoot" "$PyxsisCaptureRoot"
    nsExec::ExecToStack '"$INSTDIR\pyxsis-cli.exe" configure --capture-root "$PyxsisCaptureRoot"'
    Pop $0
    Pop $1
    ${If} $0 != 0
      MessageBox MB_ICONEXCLAMATION "Pyxsis was installed, but could not create your capture folders. Open Settings to choose a writable folder."
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  ; Silent installation skips pages, so it still needs the default check.
  Call PyxsisCheckInstallLocation
  SetOutPath "$INSTDIR"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToStack '"$INSTDIR\pyxsis-cli.exe" installer-path --remove'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "Could not remove the installer-owned user PATH entry: $1"
    MessageBox MB_ICONEXCLAMATION "Pyxsis could not remove its user PATH entry. You can remove the installation folder from PATH manually." /SD IDOK
  ${EndIf}
!macroend

; Captures and preferences intentionally survive uninstall and passive updates.
