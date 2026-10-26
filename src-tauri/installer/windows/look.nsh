; Eckermann's pages for installer.nsi, dark and in the app's colours; Tauri includes this as installerHooks

!define GROUND 1A1A1B
!define INK ECEBE8
!define MUTED 96938E
!define ART "${__FILEDIR__}\..\art"
; COLORREF is blue, green, red: the accent D9A650 and a track a shade above the ground
!define BAR 0x50A6D9
!define TRACK 0x2C2B2A
!define /ifndef PBM_SETBARCOLOR 0x0409
!define /ifndef PBM_SETBKCOLOR 0x2001

!define MUI_BGCOLOR ${GROUND}
!define MUI_TEXTCOLOR ${INK}
!define MUI_CUSTOMFUNCTION_GUIINIT Look
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.Look

ShowInstDetails nevershow
ShowUninstDetails nevershow

Var TitleFont
Var BodyFont
; the orb picture for the screen's scaling
Var Orb
Var DeleteAppDataCheckbox
Var DeleteAppDataCheckboxState

!macro HIDE ID
  GetDlgItem $0 $HWNDPARENT ${ID}
  ShowWindow $0 ${SW_HIDE}
!macroend

!macro DARK_BUTTON ID
  GetDlgItem $0 $HWNDPARENT ${ID}
  System::Call 'uxtheme::SetWindowTheme(p r0, w "DarkMode_Explorer", p 0)'
!macroend

!macro LOOK UN
  Function ${UN}Look
    InitPluginsDir
    File "/oname=$PLUGINSDIR\orb-96.bmp" "${ART}\orb-96.bmp"
    File "/oname=$PLUGINSDIR\orb-120.bmp" "${ART}\orb-120.bmp"
    File "/oname=$PLUGINSDIR\orb-144.bmp" "${ART}\orb-144.bmp"
    File "/oname=$PLUGINSDIR\orb-168.bmp" "${ART}\orb-168.bmp"
    File "/oname=$PLUGINSDIR\orb-192.bmp" "${ART}\orb-192.bmp"
    System::Call 'user32::GetDpiForWindow(p $HWNDPARENT) i .r0'
    ${If} $0 < 108
      StrCpy $Orb 96
    ${ElseIf} $0 < 132
      StrCpy $Orb 120
    ${ElseIf} $0 < 156
      StrCpy $Orb 144
    ${ElseIf} $0 < 180
      StrCpy $Orb 168
    ${Else}
      StrCpy $Orb 192
    ${EndIf}
    StrCpy $Orb "$PLUGINSDIR\orb-$Orb.bmp"

    ; DWMWA_USE_IMMERSIVE_DARK_MODE, documented from windows 11; older systems ignore it
    System::Call 'dwmapi::DwmSetWindowAttribute(p $HWNDPARENT, i 20, *i 1, i 4)'
    SetCtlColors $HWNDPARENT ${INK} ${GROUND}
    ; branding, the lines and the header icon
    !insertmacro HIDE 1028
    !insertmacro HIDE 1256
    !insertmacro HIDE 1035
    !insertmacro HIDE 1036
    !insertmacro HIDE 1045
    !insertmacro HIDE 1039
    ; undocumented but what explorer uses; without it the buttons stay light and still work
    !insertmacro DARK_BUTTON 1
    !insertmacro DARK_BUTTON 2

    CreateFont $TitleFont "Segoe UI" 20 600
    CreateFont $BodyFont "Segoe UI" 10 400
    CreateFont $1 "Segoe UI" 12 600
    GetDlgItem $0 $HWNDPARENT 1037
    SendMessage $0 ${WM_SETFONT} $1 0
    GetDlgItem $0 $HWNDPARENT 1038
    SetCtlColors $0 ${MUTED} ${GROUND}
    SendMessage $0 ${WM_SETFONT} $BodyFont 0
  FunctionEnd
!macroend
!insertmacro LOOK ""
!insertmacro LOOK "un."

!macro HEADER SHOW
  GetDlgItem $1 $HWNDPARENT 1037
  ShowWindow $1 ${SHOW}
  GetDlgItem $1 $HWNDPARENT 1038
  ShowWindow $1 ${SHOW}
!macroend

!macro LABEL X Y W H TEXT COLOR FONT
  ${NSD_CreateLabel} ${X} ${Y} ${W} ${H} "${TEXT}"
  Pop $1
  SetCtlColors $1 ${COLOR} ${GROUND}
  SendMessage $1 ${WM_SETFONT} ${FONT} 0
!macroend

; a page over the whole window, header included, with the orb, a title and two texts
!macro HERO TITLE TEXT NOTE
  nsDialogs::Create 1044
  Pop $0
  SetCtlColors $0 ${INK} ${GROUND}
  ; hidden, or alt shows them through the page
  !insertmacro HEADER ${SW_HIDE}
  ${NSD_CreateBitmap} 24u 26u 0 0 ""
  Pop $1
  ${NSD_SetImage} $1 $Orb $2
  !insertmacro LABEL 108u 28u 200u 24u "${TITLE}" ${INK} $TitleFont
  !insertmacro LABEL 108u 56u 200u 40u "${TEXT}" ${INK} $BodyFont
  !if "${NOTE}" != ""
    !insertmacro LABEL 108u 100u 200u 40u "${NOTE}" ${MUTED} $BodyFont
  !endif
!macroend

!macro BUTTON ID TEXT
  GetDlgItem $1 $HWNDPARENT ${ID}
  SendMessage $1 ${WM_SETTEXT} 0 "STR:${TEXT}"
!macroend

; next takes over back's place, which nsis shows again on every page
!macro WIDE_NEXT
  GetDlgItem $1 $HWNDPARENT 3
  GetDlgItem $2 $HWNDPARENT 1
  System::Call '*(i, i, i, i) p .r3'
  System::Call 'user32::GetWindowRect(p r1, p r3)'
  System::Call 'user32::MapWindowPoints(p 0, p $HWNDPARENT, p r3, i 2)'
  System::Call '*$3(i .r4, i .r5)'
  System::Call 'user32::GetWindowRect(p r2, p r3)'
  System::Call 'user32::MapWindowPoints(p 0, p $HWNDPARENT, p r3, i 2)'
  System::Call '*$3(i, i, i .r6, i .r7)'
  System::Free $3
  IntOp $6 $6 - $4
  IntOp $7 $7 - $5
  ShowWindow $1 ${SW_HIDE}
  ; SWP_NOZORDER | SWP_NOACTIVATE
  System::Call 'user32::SetWindowPos(p r2, p 0, i r4, i r5, i r6, i r7, i 0x14)'
!macroend

Function Welcome
  Call SkipIfPassive
  !insertmacro HERO "${PRODUCTNAME}" "$(welcomeText)" "$(welcomeNote)"
  !insertmacro BUTTON 1 "$(install)"
  nsDialogs::Show
FunctionEnd

Function Finish
  Call SkipIfPassive
  !insertmacro HERO "$(finishTitle)" "$(finishText)" ""
  !insertmacro WIDE_NEXT
  !insertmacro BUTTON 1 "$(startApp)"
  !insertmacro BUTTON 2 "$(close)"
  ; nsis disables cancel once the files are in; here it only closes
  GetDlgItem $1 $HWNDPARENT 2
  EnableWindow $1 1
  nsDialogs::Show
FunctionEnd

Function FinishLeave
  Call RunMainBinary
FunctionEnd

!macro INSTFILES UN
  Function ${UN}InstFilesShow
    !insertmacro HEADER ${SW_SHOW}
    !insertmacro HIDE 3
    FindWindow $0 "#32770" "" $HWNDPARENT
    SetCtlColors $0 ${INK} ${GROUND}
    GetDlgItem $1 $0 1006
    SetCtlColors $1 ${MUTED} ${GROUND}
    SendMessage $1 ${WM_SETFONT} $BodyFont 0
    ; un-themed, as the theme ignores the bar's colours
    GetDlgItem $1 $0 1004
    System::Call 'uxtheme::SetWindowTheme(p r1, w " ", w " ")'
    SendMessage $1 ${PBM_SETBARCOLOR} 0 ${BAR}
    SendMessage $1 ${PBM_SETBKCOLOR} 0 ${TRACK}
  FunctionEnd
!macroend
!insertmacro INSTFILES ""
!insertmacro INSTFILES "un."

; the template's reinstall page; a themed radio button ignores the text colour, so these are un-themed
!macro DARK_CHOICES PAGE TEXT FIRST SECOND
  !insertmacro HEADER ${SW_SHOW}
  SetCtlColors ${PAGE} ${INK} ${GROUND}
  SetCtlColors ${TEXT} ${INK} ${GROUND}
  System::Call 'uxtheme::SetWindowTheme(p ${FIRST}, w " ", w " ")'
  System::Call 'uxtheme::SetWindowTheme(p ${SECOND}, w " ", w " ")'
  SetCtlColors ${FIRST} ${INK} ${GROUND}
  SetCtlColors ${SECOND} ${INK} ${GROUND}
!macroend

Function un.Confirm
  Call un.SkipIfPassive
  !insertmacro HERO "$(removeTitle)" "$(removeText)" ""
  ; a themed check box ignores the text colour, so its words are a label beside it
  ${NSD_CreateCheckbox} 108u 101u 10u 10u ""
  Pop $DeleteAppDataCheckbox
  SetCtlColors $DeleteAppDataCheckbox ${INK} ${GROUND}
  System::Call 'uxtheme::SetWindowTheme(p $DeleteAppDataCheckbox, w "DarkMode_Explorer", p 0)'
  !insertmacro LABEL 122u 100u 186u 24u "$(deleteAppData)" ${MUTED} $BodyFont
  ${NSD_OnClick} $1 un.ToggleDelete
  !insertmacro BUTTON 1 "$(remove)"
  nsDialogs::Show
FunctionEnd

Function un.ToggleDelete
  ${NSD_GetState} $DeleteAppDataCheckbox $0
  ${If} $0 == ${BST_CHECKED}
    ${NSD_Uncheck} $DeleteAppDataCheckbox
  ${Else}
    ${NSD_Check} $DeleteAppDataCheckbox
  ${EndIf}
FunctionEnd

Function un.ConfirmLeave
  ${NSD_GetState} $DeleteAppDataCheckbox $DeleteAppDataCheckboxState
FunctionEnd
