import type { Settings } from "../../types/dashboard";

/** Há alterações nas listas ou nas configurações de privacidade que ainda NÃO foram aplicadas? */
export function isDirty(saved: Settings | null, draft: Settings | null): boolean {
  return saved !== null && draft !== null && JSON.stringify(saved) !== JSON.stringify(draft);
}
