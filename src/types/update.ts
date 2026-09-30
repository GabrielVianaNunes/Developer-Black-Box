/** Estado da verificação de atualizações (espelha `UpdateDto` do backend). */
export interface UpdateState {
  /** Verificação automática ligada. */
  enabled: boolean;
  current: string;
  available: boolean;
  latest: string | null;
  checkedUtcMs: number | null;
  /** Código de erro do backend (ex. "update.network"); a tela o traduz. */
  error: string | null;
  checking: boolean;
  /** Baixando e verificando o instalador. */
  downloading: boolean;
  /** Instalador baixado e verificado (SHA-256 + assinatura), pronto para instalar. */
  ready: boolean;
}
