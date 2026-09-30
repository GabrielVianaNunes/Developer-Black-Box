import type { AppCandidate } from "../../types/dashboard";

export const MAX_OPTIONS = 8;

/**
 * Filtra os programas pelo que foi digitado (nome amigável ou nome do .exe), sem repetir os que já estão na
 * lista. Quem COMEÇA com o texto vem antes de quem só o contém; dentro disso vale a ordem recebida
 * (instalados primeiro). Sem texto, mostra os primeiros.
 */
export function filterCandidates(list: AppCandidate[], query: string, taken: string[], limit = MAX_OPTIONS): AppCandidate[] {
  const q = query.trim().toLowerCase();
  const free = list.filter((c) => !taken.includes(c.exe));
  if (!q) return free.slice(0, limit);
  const starts: AppCandidate[] = [];
  const contains: AppCandidate[] = [];
  for (const c of free) {
    const name = c.name.toLowerCase();
    if (name.startsWith(q) || c.exe.startsWith(q)) starts.push(c);
    else if (name.includes(q) || c.exe.includes(q)) contains.push(c);
  }
  return [...starts, ...contains].slice(0, limit);
}
