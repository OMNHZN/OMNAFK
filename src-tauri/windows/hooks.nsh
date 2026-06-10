!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Writing OMNAFK installer metadata"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "InstallFlavor" "Branded NSIS current-user"
  WriteRegStr SHCTX "${MANUPRODUCTKEY}" "UpdateSource" "GitHub Releases"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "OMNAFK uninstall metadata cleaned"
!macroend
