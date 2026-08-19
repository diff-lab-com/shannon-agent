; NSIS installer hooks for shannon-desktop — wired via
; bundle.windows.nsis.installerHooks in desktop/tauri.conf.json.
;
; Purpose (ADR-0011 / Phase B B2): the Windows bundle carries the `shannon`
; CLI as a Tauri externalBin, extracted next to shannon-desktop.exe in
; $INSTDIR. These hooks register $INSTDIR on the PATH so `shannon` works from
; any shell.
;
; Rules:
;   - per-user: HKCU Environment\Path (matches the currentUser install mode);
;   - append-only + non-shadowing: if `where shannon` already resolves, PATH
;     is left untouched — an existing install (e.g. via install.ps1) keeps
;     winning;
;   - idempotent: re-installing over an older desktop build re-appends
;     without duplicating the entry.
;
; Only stock NSIS constructs are used (no EnVar plugin — the Tauri NSIS
; distribution does not ship it; WordFunc.nsh is already included by the
; official installer template, which also puts LogicLib in scope).

!include "WinMessages.nsh"

!macro NSIS_HOOK_POSTINSTALL
  nsExec::ExecToStack 'where shannon'
  Pop $0 ; process exit code: 0 = an existing shannon already resolves
  ${If} $0 == 0
    DetailPrint "shannon already resolvable on PATH - leaving PATH unchanged (non-shadowing)"
  ${Else}
    ReadRegStr $1 HKCU "Environment" "Path"
    ; Strip any earlier copy of $INSTDIR so re-installs don't duplicate it.
    ; WordReplace works on ;-delimited entries ("+" = case-sensitive).
    ${WordReplace} "$1" "$INSTDIR;" "" "+" $2
    ${WordReplace} "$2" ";$INSTDIR" "" "+" $3
    ${If} $3 == ""
    ${OrIf} $3 == "$INSTDIR"
      StrCpy $4 "$INSTDIR"
    ${Else}
      StrCpy $4 "$3;$INSTDIR"
    ${EndIf}
    WriteRegExpandStr HKCU "Environment" "Path" "$4"
    ; Let running shells pick up the change without a re-login.
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
    DetailPrint "Appended $INSTDIR to the user PATH (existing entries keep precedence)"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Remove exactly our $INSTDIR entry; every other PATH entry is untouched.
  ; Best-effort housekeeping: uninstall must never fail because of PATH.
  ReadRegStr $1 HKCU "Environment" "Path"
  ${If} $1 != ""
    ${WordReplace} "$1" "$INSTDIR;" "" "+" $2
    ${WordReplace} "$2" ";$INSTDIR" "" "+" $3
    ${If} $3 == "$INSTDIR"
      StrCpy $3 ""
    ${EndIf}
    WriteRegExpandStr HKCU "Environment" "Path" "$3"
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
    DetailPrint "Removed $INSTDIR from the user PATH"
  ${EndIf}
!macroend
