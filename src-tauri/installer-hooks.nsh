; Pulse became Dipstick in 0.1.10. To Windows that is a different app, so
; Pulse's updater installs Dipstick beside it. After that install, this
; carries over what Pulse had here and removes Pulse:
;
; - its login entry and its shortcuts, made again for Dipstick;
; - Pulse itself, with its own uninstaller;
; - Pulse's WebView2 folder, which holds nothing Dipstick needs.
;
; Pulse's settings and accounts are not touched here. Dipstick moves that
; folder itself on its first start, with one rename (paths.rs). Copying it
; would leave two copies of each login, and a renewed login makes the other
; copy useless.

!define OLD_PRODUCTNAME "Pulse"
!define OLD_BUNDLEID "io.github.qunqin24.PulseWindows"
!define OLD_UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${OLD_PRODUCTNAME}"
!define OLD_MANUPRODUCTKEY "Software\${MANUFACTURER}\${OLD_PRODUCTNAME}"
!define RUNKEY "Software\Microsoft\Windows\CurrentVersion\Run"

!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $R5 HKCU "${OLD_UNINSTKEY}" "UninstallString"
  ${If} $R5 != ""
    ; Started at login before, so it still is. Pulse's uninstaller takes its own entry away.
    ReadRegStr $R6 HKCU "${RUNKEY}" "${OLD_PRODUCTNAME}"
    ${If} $R6 != ""
      WriteRegStr HKCU "${RUNKEY}" "${PRODUCTNAME}" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\""
    ${EndIf}

    ; An update makes no shortcuts of its own, so the ones Pulse had are made here.
    ${If} ${FileExists} "$DESKTOP\${OLD_PRODUCTNAME}.lnk"
    ${AndIfNot} ${FileExists} "$DESKTOP\${PRODUCTNAME}.lnk"
      CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$DESKTOP\${PRODUCTNAME}.lnk"
    ${EndIf}
    ${If} ${FileExists} "$SMPROGRAMS\${OLD_PRODUCTNAME}.lnk"
    ${AndIfNot} ${FileExists} "$SMPROGRAMS\${PRODUCTNAME}.lnk"
      CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\${MAINBINARYNAME}.exe"
      !insertmacro SetLnkAppUserModelId "$SMPROGRAMS\${PRODUCTNAME}.lnk"
    ${EndIf}

    ; Pulse's own uninstaller, silent, run in place so this waits for it. It
    ; closes Pulse if it is still running. Skipped if Dipstick was installed
    ; into Pulse's folder: the uninstaller there is Dipstick's now.
    ReadRegStr $R7 HKCU "${OLD_MANUPRODUCTKEY}" ""
    ${If} $R7 != ""
    ${AndIf} $R7 != $INSTDIR
    ${AndIf} ${FileExists} "$R7\uninstall.exe"
      ExecWait '"$R7\uninstall.exe" /S _?=$R7'
      Delete "$R7\uninstall.exe"
      RMDir "$R7"
      ReadRegStr $R8 HKCU "${OLD_UNINSTKEY}" "UninstallString"
      ${If} $R8 == ""
        DeleteRegKey HKCU "${OLD_MANUPRODUCTKEY}"
        RMDir /r "$LOCALAPPDATA\${OLD_BUNDLEID}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
