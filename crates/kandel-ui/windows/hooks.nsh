; NSIS installer hooks for Kandel.
;
; Kandel ships two executables:
;   * `kandel.exe`    — the core: resident tray icon + process supervisor.
;   * `kandel-ui.exe` — the management UI, launched by the core on demand.
;
; The core locates the UI *beside itself*, so both must live in $INSTDIR. Tauri's
; `externalBin` handles that: it installs the sidecar as plain `kandel.exe`.
;
; What Tauri cannot know is which binary is the entry point. Its template sets
; MAINBINARYNAME to the Tauri app (`kandel-ui`), so every shortcut it creates —
; Start Menu, desktop, and the "run after install" action — launches the UI
; directly. That produces a window with no supervisor behind it: no tray icon,
; no process monitoring. The user is meant to launch the CORE, which shows the
; tray and opens the UI when asked.
;
; These hooks repoint the shortcuts at the core. They run after the template's
; own shortcut calls, so they overwrite the targets it just wrote.

; ── Wizard theme ─────────────────────────────────────────────────────────────
; Match the installer to the app's design system (see DESIGN.md). This file is
; included before the first `MUI_PAGE_*`, which is where MUI2 latches its
; defaults, so these `!define`s win over NSIS's grey/white ones.
;
; Colours are `RRGGBB` hexadecimal. The palette is the app's own:
;   canvas    #ffffff — page ground
;   parchment #f5f5eb — the wizard's own surface
;   ink       #14140f — all text
; The header and sidebar bitmaps (windows/header.bmp, windows/sidebar.bmp) are
; generated from the same tokens by scripts/make_icons.py, so the art and these
; colours cannot drift apart.
!define MUI_BGCOLOR "F5F5EB"        ; parchment: header, Welcome and Finish pages
!define MUI_TEXTCOLOR "14140F"      ; ink: all wizard text
!define MUI_INSTFILESPAGE_COLORS "14140F F5F5EB"  ; details: ink on parchment
!define MUI_INSTFILESPAGE_PROGRESSBAR "colored"   ; progress bar in the lime accent

; ── Shortcuts ────────────────────────────────────────────────────────────────

!macro kandel_point_shortcuts_at_core
  ; Start Menu shortcut. The template may nest it under a folder when
  ; STARTMENUFOLDER is set, so cover both layouts.
  IfFileExists "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" 0 +2
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk" "$INSTDIR\kandel.exe"
  IfFileExists "$SMPROGRAMS\${PRODUCTNAME}.lnk" 0 +2
    CreateShortcut "$SMPROGRAMS\${PRODUCTNAME}.lnk" "$INSTDIR\kandel.exe"

  ; Desktop shortcut, when the user asked for one.
  IfFileExists "$DESKTOP\${PRODUCTNAME}.lnk" 0 +2
    CreateShortcut "$DESKTOP\${PRODUCTNAME}.lnk" "$INSTDIR\kandel.exe"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro kandel_point_shortcuts_at_core

  ; Also point the uninstall metadata at the core so Explorer shows the right
  ; icon and "Open file location" lands on the entry point.
  WriteRegStr SHCTX "${UNINSTKEY}" "DisplayIcon" "$\"$INSTDIR\kandel.exe$\""
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; The template removes its own shortcuts; clear ours too in case a name
  ; differs, so an uninstall leaves nothing behind.
  Delete "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  Delete "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  Delete "$DESKTOP\${PRODUCTNAME}.lnk"
!macroend
