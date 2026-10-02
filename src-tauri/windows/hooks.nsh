; Ganchos do instalador NSIS do Developer Black Box.
;
; Ao desinstalar, remove a entrada de "abrir com o Windows" (chave Run do usuário atual) para não
; deixar um atalho de inicialização apontando para um programa que não existe mais.
;
; Os dados gravados (chave DPAPI, banco e segmentos cifrados) ficam em
; %LOCALAPPDATA%\DeveloperBlackBox e NÃO são apagados na desinstalação: são evidências suas.
; Para apagá-los, use "Excluir tudo" na aba Armazenamento antes de desinstalar, ou apague a pasta.

; Pasta de instalação: o padrão (%LOCALAPPDATA%\Programs\Developer Black Box) já é o recomendado e a página de escolha
; de pasta continua disponível. Antes de copiar qualquer arquivo, recusamos as pastas que sabemos que não funcionam:
;   * Program Files (qualquer variante): este instalador é só do usuário e não tem permissão de administrador.
;   * Diretamente dentro de %LOCALAPPDATA% ou %APPDATA% (ex.: %LOCALAPPDATA%\Developer Black Box): no teste feito em um
;     PC o Windows mostrou o ícone genérico para o programa nessas pastas. Subpastas mais fundas (Programs\...) funcionam.
; Atualizações de uma instalação que já existe (/UPDATE) NUNCA são recusadas: quem já está numa pasta dessas não pode ficar
; sem receber atualizações. A recusa não apaga nada: só remove a pasta vazia que o instalador acabou de criar.
!macro BB_UNDER_DIR base
  ${If} $R8 = 0
    StrLen $R1 "${base}\"
    StrCpy $R2 $INSTDIR $R1
    ${If} $R2 == "${base}\"
    ${OrIf} $INSTDIR == "${base}"
      StrCpy $R8 1
    ${EndIf}
  ${EndIf}
!macroend

; Pasta que está DIRETAMENTE dentro de `base` (sem nenhuma outra pasta no meio).
!macro BB_DIRECT_CHILD base
  ${If} $R8 = 0
    StrLen $R1 "${base}\"
    StrCpy $R2 $INSTDIR $R1
    ${If} $R2 == "${base}\"
      StrCpy $R3 $INSTDIR "" $R1
      StrCpy $R4 0
      bb_scan_${base}:
        StrCpy $R5 $R3 1 $R4
        StrCmp $R5 "" bb_scan_done_${base}
        StrCmp $R5 "\" bb_scan_done_${base}
        IntOp $R4 $R4 + 1
        Goto bb_scan_${base}
      bb_scan_done_${base}:
      ${If} $R5 == ""
        StrCpy $R8 2
      ${EndIf}
    ${ElseIf} $INSTDIR == "${base}"
      StrCpy $R8 2
    ${EndIf}
  ${EndIf}
!macroend

!macro BB_CHECK_INSTDIR
  StrCpy $R8 0
  ${If} $UpdateMode <> 1
    !insertmacro BB_UNDER_DIR "$PROGRAMFILES"
    !insertmacro BB_UNDER_DIR "$PROGRAMFILES32"
    !insertmacro BB_UNDER_DIR "$PROGRAMFILES64"
    !insertmacro BB_DIRECT_CHILD "$LOCALAPPDATA"
    !insertmacro BB_DIRECT_CHILD "$APPDATA"
    ${If} $R8 <> 0
      ${If} $LANGUAGE = 1046
        ${If} $R8 = 1
          StrCpy $R9 "Esta pasta (Arquivos de Programas) exige permissão de administrador e este instalador é só para o seu usuário. Volte e escolha outra pasta, de preferência a sugerida."
        ${Else}
          StrCpy $R9 "Nesta pasta o Windows pode mostrar o ícone genérico do programa. Volte e escolha outra pasta, de preferência a sugerida (a que termina em Programs\Developer Black Box)."
        ${EndIf}
      ${Else}
        ${If} $R8 = 1
          StrCpy $R9 "This folder (Program Files) needs administrator rights and this installer is per-user. Go back and pick another folder, preferably the suggested one."
        ${Else}
          StrCpy $R9 "In this folder Windows can show a generic program icon. Go back and pick another folder, preferably the suggested one (ending in Programs\Developer Black Box)."
        ${EndIf}
      ${EndIf}
      MessageBox MB_ICONSTOP|MB_OK "$R9" /SD IDOK
      RMDir "$INSTDIR"
      Abort
    ${EndIf}
  ${EndIf}
!macroend

; Ícone antigo em cache. Até a 0.3.3 o ícone do executável trazia uma luz cinza ao lado do cubo; desde a 0.3.4 é só o cubo.
; O Windows guarda o ícone antigo na MEMÓRIA do Explorer e nenhum aviso (SHChangeNotify, ie4uinit, atalho regravado) o
; invalida; medido em laboratório: só reiniciar o Explorer renova. Reiniciar o Explorer fecha por alguns segundos a barra
; de tarefas e as janelas do Explorador de Arquivos, por isso só acontece SE a pessoa aceitar e SÓ ao atualizar de uma
; versão com o ícone antigo. Em instalação silenciosa a resposta padrão é "Não".
Var BBIconRefresh

; Entrada: $R0 = versão anterior ("" se não havia). Saída: $BBIconRefresh = 1 se ela tinha o ícone antigo (0.1.x, 0.2.x,
; 0.3.0 a 0.3.3), senão 0. Usa $R1.
!macro BB_CLASSIFY_PREVIOUS_ICON
  StrCpy $BBIconRefresh 0
  StrCpy $R1 $R0 4
  ${If} $R1 == "0.1."
  ${OrIf} $R1 == "0.2."
    StrCpy $BBIconRefresh 1
  ${EndIf}
  StrLen $R1 $R0
  ${If} $R1 = 5
    ${If} $R0 == "0.3.0"
    ${OrIf} $R0 == "0.3.1"
    ${OrIf} $R0 == "0.3.2"
    ${OrIf} $R0 == "0.3.3"
      StrCpy $BBIconRefresh 1
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro BB_CHECK_INSTDIR
  ; versão instalada ANTES desta (lida antes de o instalador gravar a nova)
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  !insertmacro BB_CLASSIFY_PREVIOUS_ICON
!macroend

; Depois de instalar ou atualizar: pede ao Windows que renove os ícones (SHChangeNotify com SHCNE_ASSOCCHANGED; não altera
; nenhum arquivo) e, se a versão anterior tinha o ícone antigo, oferece reiniciar o Explorer para renová-lo de verdade.
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
  ${If} $BBIconRefresh = 1
    ${If} $LANGUAGE = 1046
      StrCpy $R9 "Esta atualização troca o ícone do aplicativo, mas o Windows guarda o ícone antigo na memória do Explorador de Arquivos.$\r$\n$\r$\nReiniciar o Explorador de Arquivos agora? A barra de tarefas e as janelas do Explorador de Arquivos fecham por alguns segundos. Nada é perdido. Se você escolher Não, o ícone novo aparece depois de reiniciar o Windows."
    ${Else}
      StrCpy $R9 "This update changes the app icon, but Windows keeps the old icon in memory inside File Explorer.$\r$\n$\r$\nRestart File Explorer now? The taskbar and any open File Explorer windows close for a few seconds. Nothing is lost. If you choose No, the new icon appears after you restart Windows."
    ${EndIf}
    MessageBox MB_YESNO|MB_ICONQUESTION "$R9" /SD IDNO IDNO bb_skip_explorer_restart
    nsExec::Exec 'taskkill /f /im explorer.exe'
    Sleep 1500
    Exec 'explorer.exe'
    bb_skip_explorer_restart:
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DeveloperBlackBox"
!macroend
