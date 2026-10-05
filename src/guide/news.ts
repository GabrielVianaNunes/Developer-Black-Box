import type { Step } from "./steps";

/** O resumo de uma versão: só entram versões que mudam o uso do app (correções e ajustes internos ficam de fora). */
export interface Release {
  version: string;
  steps: Step[];
}

/**
 * O conteúdo das novidades fica DENTRO do app (nos dois idiomas): nada é buscado na rede. Ordem: da mais antiga
 * para a mais nova.
 */
export const RELEASES: Release[] = [
  {
    version: "0.3.0",
    steps: [
      { id: "picker", title: "guide.news.picker.title", body: ["guide.news.picker.b1", "guide.news.picker.b2"] },
      { id: "rules", title: "guide.news.rules.title", body: ["guide.news.rules.b1", "guide.news.rules.b2"] },
      { id: "done", title: "guide.news.guide.title", body: ["guide.news.guide.b1"] },
    ],
  },
  {
    version: "0.4.0",
    steps: [{ id: "rules", title: "guide.news.clarity.title", body: ["guide.news.clarity.b1", "guide.news.clarity.b2"] }],
  },
  {
    version: "0.5.0",
    steps: [{ id: "incidents", title: "guide.news.health.title", body: ["guide.news.health.b1", "guide.news.health.b2"] }],
  },
  {
    version: "0.6.0",
    steps: [{ id: "privacy", title: "guide.news.switches.title", body: ["guide.news.switches.b1"] }],
  },
];

/** Só a parte numérica: "0.3.0-rc.1" conta como 0.3.0. Texto que não é versão vira null. */
export function parseVersion(v: string): [number, number, number] | null {
  const m = /^(\d+)\.(\d+)\.(\d+)(?:[-+].*)?$/.exec(v.trim());
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}

export function compareVersions(a: [number, number, number], b: [number, number, number]): number {
  for (let i = 0; i < 3; i++) if (a[i] !== b[i]) return a[i] < b[i] ? -1 : 1;
  return 0;
}

/**
 * Os passos das novidades que a pessoa ainda não viu: as versões depois da última vista (`seen`, ou todas se nunca
 * viu) até a versão atual, da mais antiga para a mais nova. Sem versão atual legível não mostra nada.
 */
export function newsFor(seen: string | null, current: string, releases: Release[] = RELEASES): Step[] {
  const cur = parseVersion(current);
  if (!cur) return [];
  const last = seen === null ? null : parseVersion(seen);
  return releases
    .filter((r) => {
      const v = parseVersion(r.version);
      return v !== null && compareVersions(v, cur) <= 0 && (last === null || compareVersions(v, last) > 0);
    })
    .flatMap((r) => r.steps);
}
