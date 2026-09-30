; Ganchos do instalador NSIS do Developer Black Box.
;
; Ao desinstalar, remove a entrada de "abrir com o Windows" (chave Run do usuário atual) para não
; deixar um atalho de inicialização apontando para um programa que não existe mais.
;
; Os dados gravados (chave DPAPI, banco e segmentos cifrados) ficam em
; %LOCALAPPDATA%\DeveloperBlackBox e NÃO são apagados na desinstalação: são evidências suas.
; Para apagá-los, use "Excluir tudo" na aba Armazenamento antes de desinstalar, ou apague a pasta.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "DeveloperBlackBox"
!macroend
