; Installer hooks (tauri.conf.json bundle.windows.nsis.installerHooks).

; Before the rename to aina the app was installed as "B.A.D.I." (its own folder and uninstall entry), so
; installing aina would leave two apps side by side. The old copy is uninstalled silently first. Its
; uninstaller keeps the settings, login and agents in %APPDATA%\BADI, which aina goes on using.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\B.A.D.I." "InstallLocation"
  ${If} $R0 != ""
    ; Stored with quotes: "C:\Users\…\AppData\Local\B.A.D.I."
    StrCpy $R1 $R0 1
    ${If} $R1 == '"'
      StrCpy $R0 $R0 "" 1
      StrCpy $R0 $R0 -1
    ${EndIf}
    ${If} ${FileExists} "$R0\uninstall.exe"
      DetailPrint "Removing the earlier B.A.D.I. install"
      ; _?= runs the uninstaller in place and waits for it.
      ExecWait '"$R0\uninstall.exe" /S _?=$R0'
      Delete "$R0\uninstall.exe"
      RMDir "$R0"
    ${EndIf}
    DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\B.A.D.I."
  ${EndIf}
!macroend
