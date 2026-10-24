; WhisperTube NSIS lifecycle hooks.
; The application-local data directory is intentionally kept by default so a
; reinstall does not force users to download their models again.

Var WhisperTubeRemoveUserData

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

  ; Keep the deletion scope exact. Do not remove the entire app-local-data
  ; root because WebView/config data may be owned by other lifecycle paths.
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\models"
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\jobs"
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\runtime"
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\cookie-jars"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db"
  ; Never discard recovery files when the main database is still in use.
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db" whispertube_cleanup_failed 0
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-journal"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-wal"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db-shm"

  IfFileExists "$LOCALAPPDATA\app.whispertube.local\models\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\jobs\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\runtime\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\cookie-jars\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-journal" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-wal" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db-shm" whispertube_cleanup_failed 0
  Goto whispertube_uninstall_done

  whispertube_cleanup_failed:
    MessageBox MB_OK|MB_ICONEXCLAMATION \
      "WhisperTube was uninstalled, but some user data could not be removed. Close WhisperTube and delete the remaining data from:$\r$\n$\r$\n$LOCALAPPDATA\app.whispertube.local"

  whispertube_uninstall_done:
!macroend
