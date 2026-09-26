; Installer hooks for the NSIS package.
;
; ACE-Step Studio 1.x was a Python and Node.js folder, not an installed
; program, so there is nothing of it to remove here.

!macro NSIS_HOOK_PREINSTALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The studio keeps its models beside the executable rather than in the user
  ; profile, so nothing lands on C: unless that is where it was installed. The
  ; uninstaller's "delete application data" only knows about the profile, which
  ; is why ticking it still left the installation folder behind with ten or
  ; twenty gigabytes of weights in it. Ticked means ticked: the data folder goes
  ; too, and then the directory itself, which until now could never be empty.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RMDir /r "$INSTDIR\data"
  ${EndIf}

  ; Empty-only, so an unticked uninstall still leaves the weights alone. This is
  ; what removes the folder itself once nothing is left in it: the template tries
  ; before the hooks run, when the data folder is still there.
  RMDir "$INSTDIR"
!macroend
