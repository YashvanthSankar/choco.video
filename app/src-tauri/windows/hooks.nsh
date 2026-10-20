; NSIS hooks for the Windows installer (docs/TECH.md §19.7).
; Models live in the install folder ($LOCALAPPDATA\choco.video\models) so the CLI and the app share
; them. The uninstaller removes only what it installed, so models survive upgrades and plain
; uninstalls; ticking "Delete the application data" also removes the models and the GPU check.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\models"
    Delete "$INSTDIR\gpu-check.json"
    RMDir "$INSTDIR"
  ${EndIf}
!macroend
