; WhisperTube NSIS lifecycle hooks.
; The application-local data directory is intentionally kept by default so a
; reinstall does not force users to download their models again.

Var WhisperTubeRemoveUserData

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $WhisperTubeRemoveUserData "0"
  MessageBox MB_YESNO|MB_ICONQUESTION \
    "Remove WhisperTube user data too?$\r$\n$\r$\nThis deletes downloaded Whisper models, CUDA/Vulkan runtimes, history, saved audio, and transcription jobs. External files such as cookies.txt are not touched.$\r$\n$\r$\nChoose No to uninstall the application but keep the data for a future reinstall." \
    IDYES whispertube_remove_data IDNO whispertube_keep_data

  Goto whispertube_keep_data

  whispertube_remove_data:
    StrCpy $WhisperTubeRemoveUserData "1"

  whispertube_keep_data:
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  StrCmp $WhisperTubeRemoveUserData "1" 0 whispertube_uninstall_done

  ; Keep the deletion scope exact. Do not remove the entire app-local-data
  ; root because WebView/config data may be owned by other lifecycle paths.
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\models"
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\jobs"
  RMDir /r "$LOCALAPPDATA\app.whispertube.local\runtime"
  Delete "$LOCALAPPDATA\app.whispertube.local\whispertube.db"

  IfFileExists "$LOCALAPPDATA\app.whispertube.local\models\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\jobs\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\runtime\*.*" whispertube_cleanup_failed 0
  IfFileExists "$LOCALAPPDATA\app.whispertube.local\whispertube.db" whispertube_cleanup_failed 0
  Goto whispertube_uninstall_done

  whispertube_cleanup_failed:
    MessageBox MB_OK|MB_ICONEXCLAMATION \
      "WhisperTube was uninstalled, but some user data could not be removed. Close WhisperTube and delete the remaining data from:$\r$\n$\r$\n$LOCALAPPDATA\app.whispertube.local"

  whispertube_uninstall_done:
!macroend
