; Capture storage is independent of the application installation directory.
!include nsDialogs.nsh
!include LogicLib.nsh
Var PyxsisCaptureRoot
Var PyxsisCaptureInput
Var PyxsisCaptureDialog

; Registered before the stock Tauri pages: a small, explicit storage-choice page.
Page custom PyxsisCapturesPage PyxsisCapturesLeave

Function PyxsisCapturesPage
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

; Captures and preferences intentionally survive uninstall and passive updates.
