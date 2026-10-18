; Remove the firewall rule and the autostart entry when Moush is uninstalled.
; (The rule is added from the app's Diagnostics page, which asks for administrator approval.)
!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="Moush"'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Moush"
!macroend
