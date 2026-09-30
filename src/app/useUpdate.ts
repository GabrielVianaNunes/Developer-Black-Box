import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { downloadUpdate, getUpdateState, installUpdate, onUpdate } from "../services/backend";
import type { UpdateState } from "../types/update";

/** Estado das atualizações (lido ao abrir e mantido pelos eventos do backend) e as ações do usuário. */
export function useUpdate() {
  const { errorText } = useI18n();
  const [state, setState] = useState<UpdateState | null>(null);
  const [installing, setInstalling] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | null = null;
    getUpdateState().then((s) => alive && setState(s)).catch(() => {});
    onUpdate((s) => alive && setState(s)).then((u) => (alive ? (unlisten = u) : u()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  /** Baixa e verifica. Não instala nada. */
  const download = useCallback(async () => {
    setActionError(null);
    try {
      setState(await downloadUpdate());
    } catch (e) {
      setActionError(errorText(e));
    }
  }, [errorText]);

  /** Fecha o app e abre o instalador (só depois de um clique explícito). */
  const install = useCallback(async () => {
    setActionError(null);
    setInstalling(true);
    try {
      await installUpdate();
    } catch (e) {
      setInstalling(false);
      setActionError(errorText(e));
    }
  }, [errorText]);

  return { state, setState, download, install, installing, actionError };
}
