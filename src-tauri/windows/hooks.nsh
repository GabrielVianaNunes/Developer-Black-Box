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

!macro NSIS_HOOK_PREINSTALL
  !insertmacro BB_CHECK_INSTDIR
!macroend

; Depois de instalar ou atualizar, pede ao Windows que renove os ícones (SHChangeNotify com SHCNE_ASSOCCHANGED, a
; notificação oficial de "os ícones mudaram"). Sem isso o Windows continua mostrando no botão da barra de tarefas o
; ícone do executável que ele guardou em cache de uma versão anterior. Não altera nenhum arquivo nem configuração.
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0x1000, p 0, p 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DeveloperBlackBox"
!macroend
