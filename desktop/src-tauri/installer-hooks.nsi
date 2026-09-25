; Installer hooks for the NSIS package.
;
; The executable used to be named after the Rust crate -
; minimax-music3-studio-desktop.exe - while the portable build named the same
; binary MiniMax-Music3-Studio.exe. One studio, two names, depending on how it
; was installed. The name is now the same in both, and this removes the old one
; so an updated installation does not keep a dead 52 MB copy and a shortcut
; pointing at whichever the user happened to click first.

!macro NSIS_HOOK_PREINSTALL
  Delete "$INSTDIR\minimax-music3-studio-desktop.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\minimax-music3-studio-desktop.exe"

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
