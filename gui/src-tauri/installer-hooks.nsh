; Extra steps for the Windows installer (referenced from tauri.conf.json).
;
; The uninstaller also runs during an upgrade, and must then leave your setup alone:
; - the updater starts it with /UPDATE ($UpdateMode = 1);
; - a newer setup run by hand offers "Uninstall before installing" and runs it in place,
;   from the install folder (with _?=). Windows' "Apps" settings run Uninstall.exe without
;   _?=, so it copies itself to %TEMP% and runs from there: running from the install folder
;   therefore means an upgrade or reinstall, not a removal.
!macro NBW_SKIP_IF_UPGRADE label
  ${If} $UpdateMode = 1
  ${OrIf} $EXEDIR == $INSTDIR
    Goto ${label}
  ${EndIf}
!macroend

; Real uninstall only: remove both scheduled tasks, so they do not keep pointing at a
; deleted program, and the tray icon's login startup entry. A run in progress is stopped
; first: deleting a task does not end it, and it would keep files in the data folder open.
!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro NBW_SKIP_IF_UPGRADE nbw_pre_done
  nsExec::Exec 'schtasks /End /TN NoBloatWallpaper'
  Pop $0
  nsExec::Exec 'schtasks /End /TN NoBloatWallpaper-Login'
  Pop $0
  nsExec::Exec 'schtasks /Delete /F /TN NoBloatWallpaper'
  Pop $0
  nsExec::Exec 'schtasks /Delete /F /TN NoBloatWallpaper-Login'
  Pop $0
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "NoBloatWallpaperTray"
  nbw_pre_done:
!macroend

; "Delete the application data" ticked: the template removes the folders named after the
; bundle identifier; the app's settings, downloads and favorites live in
; %APPDATA%\no-bloat-wallpaper, so remove that too.
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro NBW_SKIP_IF_UPGRADE nbw_post_done
  ${If} $DeleteAppDataCheckboxState = 1
    SetShellVarContext current
    RmDir /r "$APPDATA\no-bloat-wallpaper"
  ${EndIf}
  nbw_post_done:
!macroend
