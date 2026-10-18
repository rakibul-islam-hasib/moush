; Remove the firewall rule, the autostart entry and the notification identity when Moush is uninstalled.
; (The rule is added from the app's Diagnostics page, which asks for administrator approval.)
!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec 'netsh advfirewall firewall delete rule name="Moush"'
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Moush"
  ; Notification identity registered by the app for toasts (see moush-engine win/toast.rs).
  DeleteRegKey HKCU "Software\Classes\AppUserModelId\app.moush.desktop"
!macroend
