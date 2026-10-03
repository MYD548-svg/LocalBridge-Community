; LocalBridge installer lifecycle hooks.
; User data is preserved by default. Interactive uninstallers ask explicitly and
; default to No; silent uninstallers require /DELETEUSERDATA=1.
!include "FileFunc.nsh"
!include "LogicLib.nsh"
Var LocalBridgeDeleteUserData

; Exact keys and values; preserve registrations owned by another installation.
!macro LocalBridgeCheckRegistration VIEW BROWSER
  SetRegView ${VIEW}
  ReadRegStr $0 HKLM "SOFTWARE\${BROWSER}\NativeMessagingHosts\com.localbridge.chatgpt_web" ""
  ${If} $0 != ""
  ${AndIf} $0 != "$INSTDIR\localbridge-native-host.json"
    MessageBox MB_OK|MB_ICONSTOP "检测到其他 LocalBridge 安装的浏览器注册：$0。请先卸载该安装。"
    Abort
  ${EndIf}
!macroend
!macro LocalBridgeRegister VIEW BROWSER
  SetRegView ${VIEW}
  ClearErrors
  WriteRegStr HKLM "SOFTWARE\${BROWSER}\NativeMessagingHosts\com.localbridge.chatgpt_web" "" "$INSTDIR\localbridge-native-host.json"
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "浏览器宿主注册失败，请重新运行安装程序。"
    Abort
  ${EndIf}
!macroend
!macro LocalBridgeUnregister VIEW BROWSER
  SetRegView ${VIEW}
  ReadRegStr $0 HKLM "SOFTWARE\${BROWSER}\NativeMessagingHosts\com.localbridge.chatgpt_web" ""
  ${If} $0 == "$INSTDIR\localbridge-native-host.json"
    DeleteRegKey HKLM "SOFTWARE\${BROWSER}\NativeMessagingHosts\com.localbridge.chatgpt_web"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro LocalBridgeCheckRegistration 32 "Microsoft\Edge"
  !insertmacro LocalBridgeCheckRegistration 32 "Google\Chrome"
  !insertmacro LocalBridgeCheckRegistration 64 "Microsoft\Edge"
  !insertmacro LocalBridgeCheckRegistration 64 "Google\Chrome"
  SetRegView 64
  ${If} ${FileExists} "$INSTDIR\localbridge-browser-host.exe"
    ExecWait '"$INSTDIR\localbridge-browser-host.exe" --prepare-update' $0
    ${If} $0 != 0
      MessageBox MB_OK|MB_ICONSTOP "请关闭浏览器中的 LocalBridge 扩展及 LocalBridge，再重新运行安装程序。原安装已保留。"
      Abort
    ${EndIf}
  ${EndIf}
!macroend
!macro NSIS_HOOK_POSTINSTALL
  ExecWait '"$INSTDIR\localbridge-browser-host.exe" --write-manifest' $0
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "浏览器宿主清单生成失败，请重新运行安装程序。"
    Abort
  ${EndIf}
  !insertmacro LocalBridgeRegister 32 "Microsoft\Edge"
  !insertmacro LocalBridgeRegister 32 "Google\Chrome"
  !insertmacro LocalBridgeRegister 64 "Microsoft\Edge"
  !insertmacro LocalBridgeRegister 64 "Google\Chrome"
  SetRegView 64
!macroend
!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro LocalBridgeUnregister 32 "Microsoft\Edge"
  !insertmacro LocalBridgeUnregister 32 "Google\Chrome"
  !insertmacro LocalBridgeUnregister 64 "Microsoft\Edge"
  !insertmacro LocalBridgeUnregister 64 "Google\Chrome"
  SetRegView 64
  Delete "$INSTDIR\localbridge-native-host.json"
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  StrCpy $LocalBridgeDeleteUserData "0"
  IfSilent LocalBridgeSilentUninstall LocalBridgePromptDeleteUserData
LocalBridgePromptDeleteUserData:
  MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "同时删除已保存的 LocalBridge Runtime API Key？" IDYES LocalBridgeDeleteUserDataNow IDNO LocalBridgeKeepUserData
LocalBridgeSilentUninstall:
  ${GetOptions} $CMDLINE "/DELETEUSERDATA=" $LocalBridgeDeleteUserData
  StrCmp $LocalBridgeDeleteUserData "1" LocalBridgeDeleteUserDataNow LocalBridgeKeepUserData
LocalBridgeDeleteUserDataNow:
  System::Call 'advapi32::CredDeleteW(w "LocalBridge/RuntimeApiKey/runtime-api-key", i 1, i 0) i .r0'
LocalBridgeKeepUserData:
!macroend
