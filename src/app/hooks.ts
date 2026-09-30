import { useCallback, useEffect, useRef, useState } from "react";

/**
 * Executa `fn` agora e a cada `ms`; devolve o último resultado e uma função para recarregar.
 * `error` é o código de erro do backend (a tela o traduz com `errorText`), nunca um texto de idioma.
 */
export function usePolling<T>(fn: () => Promise<T>, ms: number, deps: unknown[] = []) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const fnRef = useRef(fn);
  fnRef.current = fn;

  const reload = useCallback(async () => {
    try {
      setData(await fnRef.current());
      setError(null);
    } catch (e) {
      setError(typeof e === "string" ? e : "generic");
    }
  }, []);

  useEffect(() => {
    let alive = true;
    const run = () => alive && void reload();
    run();
    const t = window.setInterval(run, ms);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ms, reload, ...deps]);

  return { data, error, reload };
}
