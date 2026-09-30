import { useEffect, useState } from "react";
import { getUpdateState, onUpdate } from "../services/backend";
import type { UpdateState } from "../types/update";

/** Estado das atualizações: lido ao abrir e mantido pelos eventos do backend. */
export function useUpdate() {
  const [state, setState] = useState<UpdateState | null>(null);

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

  return { state, setState };
}
