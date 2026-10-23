; Registers Magic Mend to start at login, and removes that entry on uninstall.
!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Magic Mend" '"$INSTDIR\magic-mend.exe" --autostart'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Magic Mend"
!macroend
