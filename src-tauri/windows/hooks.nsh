; WhisperTube NSIS lifecycle hooks.
; The application-local data directory is intentionally kept by default so a
; reinstall does not force users to download their models again.

Var WhisperTubeRemoveUserData

!ifdef WHISPERTUBE_TEST_NORMAL_SECTION
  !define WHISPERTUBE_CLEANUP_PREFIX ""
!else
  !define WHISPERTUBE_CLEANUP_PREFIX "un."
!endif

; Return an open handle to an ordinary directory, or -1. No write/delete
; sharing while a caller is traversing children of the directory.
Function ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubeOpenCleanupDirectory
  Exch $0
  Push $1
  System::Call 'kernel32::CreateFileW(w r0, i 0x80, i 1, p 0, i 3, i 0x02200000, p 0) p .r1'
  IntPtrCmp $1 -1 directory_open_done
  System::Call 'kernel32::GetFileAttributesW(w r0) i .r0'
  IntCmp $0 -1 directory_open_reject
  IntOp $0 $0 & 0x410
  IntCmp $0 0x10 directory_open_done
  directory_open_reject:
    System::Call 'kernel32::CloseHandle(p r1)'
    StrCpy $1 -1
  directory_open_done:
    StrCpy $0 $1
    Pop $1
    Exch $0
FunctionEnd

; Return 0 after removing an ordinary tree or 1 if anything must be preserved.
Function ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubePurgeTree
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  StrCpy $5 0
  Push $0
  Call ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubeOpenCleanupDirectory
  Pop $1
  IntPtrCmp $1 -1 tree_open_failed
  ClearErrors
  FindFirst $2 $3 "$0\*"
  IfErrors tree_enumeration_failed

  tree_next:
    StrCmp $3 "." tree_advance
    StrCmp $3 ".." tree_advance
    System::Call 'kernel32::GetFileAttributesW(w "$0\$3") i .r4'
    IntCmp $4 -1 tree_skip_entry
    IntOp $6 $4 & 0x400
    IntCmp $6 0 0 tree_skip_entry tree_skip_entry
    IntOp $6 $4 & 0x10
    IntCmp $6 0 tree_delete_file
    Push "$0\$3"
    Call ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubePurgeTree
    Pop $6
    StrCmp $6 "0" tree_advance tree_skip_entry

  tree_delete_file:
    ClearErrors
    Delete "$0\$3"
    IfErrors tree_skip_entry tree_advance

  tree_skip_entry:
    StrCpy $5 1

  tree_advance:
    ClearErrors
    FindNext $2 $3
    IfErrors 0 tree_next
    FindClose $2
    Goto tree_close

  tree_enumeration_failed:
    StrCpy $5 1
  tree_close:
    System::Call 'kernel32::CloseHandle(p r1)'
    StrCmp $5 "0" 0 tree_done
    ClearErrors
    RMDir "$0"
    IfErrors tree_open_failed tree_done

  tree_open_failed:
    StrCpy $5 1
  tree_done:
    StrCpy $0 $5
    Pop $6
    Pop $5
    Pop $4
    Pop $3
    Pop $2
    Pop $1
    Exch $0
FunctionEnd

!macro WHISPERTUBE_PURGE_OWNED_DIR PATH
  IfFileExists "${PATH}" 0 +5
  Push "${PATH}"
  Call ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubePurgeTree
  Pop $0
  IntOp $R1 $R1 | $0
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Tauri has replaced the executable and updated shortcuts at this point.
  ; Notify Explorer so an upgrade at the same path refreshes the cached icon.
  ; SHCNE_UPDATEITEM / SHCNF_PATHW, then SHCNE_ASSOCCHANGED / SHCNF_IDLIST.
  System::Call 'shell32::SHChangeNotify(i 0x00002000, i 0x0005, w "$INSTDIR\${MAINBINARYNAME}.exe", p 0)'
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x0000, p 0, p 0)'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Use Tauri's confirmation-page checkbox as the only deletion decision.
  ; Consume it before the template reaches its whole-app-data deletion block:
  ; our POST hook owns the narrower, application-managed cleanup scope.
  StrCpy $WhisperTubeRemoveUserData "0"
  StrCmp $UpdateMode "1" whispertube_keep_data
  StrCmp $DeleteAppDataCheckboxState "1" 0 whispertube_keep_data
  StrCpy $WhisperTubeRemoveUserData "1"
  whispertube_keep_data:
  StrCpy $DeleteAppDataCheckboxState "0"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  StrCmp $WhisperTubeRemoveUserData "1" 0 whispertube_uninstall_done
  IfFileExists "$LOCALAPPDATA\app.whispertube.local" 0 whispertube_uninstall_done
  Push $R0
  Push $R1
  StrCpy $R1 0
  Push "$LOCALAPPDATA\app.whispertube.local"
  Call ${WHISPERTUBE_CLEANUP_PREFIX}WhisperTubeOpenCleanupDirectory
  Pop $R0
  IntPtrCmp $R0 -1 whispertube_cleanup_failed

  ; Keep WebView and configuration data outside the cleanup scope.
  !insertmacro WHISPERTUBE_PURGE_OWNED_DIR "$LOCALAPPDATA\app.whispertube.local\models"
  !insertmacro WHISPERTUBE_PURGE_OWNED_DIR "$LOCALAPPDATA\app.whispertube.local\jobs"
  !insertmacro WHISPERTUBE_PURGE_OWNED_DIR "$LOCALAPPDATA\app.whispertube.local\runtime"
  !insertmacro WHISPERTUBE_PURGE_OWNED_DIR "$LOCALAPPDATA\app.whispertube.local\cookie-jars"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db"
  ; Never discard recovery files when the main database is still in use.
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db" whispertube_cleanup_failed 0
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-journal"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-wal"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-shm"

  StrCmp $R1 "0" 0 whispertube_cleanup_failed
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-journal" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-wal" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-shm" whispertube_cleanup_failed 0
  Goto whispertube_cleanup_done

  whispertube_cleanup_failed:
    MessageBox MB_OK|MB_ICONEXCLAMATION \
      "WhisperTube was uninstalled, but some user data could not be removed. Close WhisperTube and delete the remaining data from:$\r$\n$\r$\n$LOCALAPPDATA\app.whispertube.local"

  whispertube_cleanup_done:
    IntPtrCmp $R0 -1 +2
    System::Call 'kernel32::CloseHandle(p $R0)'
    Pop $R1
    Pop $R0
  whispertube_uninstall_done:
!macroend
