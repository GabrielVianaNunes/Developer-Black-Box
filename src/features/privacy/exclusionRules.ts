import type { ExclusionKind, PartialExclusion, Settings } from "../../types/dashboard";

/**
 * Lógica das regras de exclusão na tela. Para o usuário a regra de um programa diz o que CONTINUA sendo gravado
 * (por padrão, nada); no backend ela é guardada como o que NÃO é gravado: o programa inteiro em `excludedApps`, ou
 * só alguns tipos em `partialExclusions`. Aqui ficam as conversões e as regras de coerência, sem nada de tela.
 */

/** O que continua sendo gravado do programa. Tudo falso = o programa está excluído por inteiro. */
export interface Recorded {
  lifecycle: boolean;
  metrics: boolean;
  crashes: boolean;
}

export interface Rule {
  exe: string;
  recorded: Recorded;
}

export const NOTHING: Recorded = { lifecycle: false, metrics: false, crashes: false };
export const KINDS: ExclusionKind[] = ["lifecycle", "metrics", "crashes"];

/** Coerência: CPU e memória só podem ser gravados com o início e fim (é o início que dá o nome do programa). */
export function coherent(r: Recorded): Recorded {
  return { lifecycle: r.lifecycle, metrics: r.metrics && r.lifecycle, crashes: r.crashes };
}

/** Gravar os três tipos é o mesmo que não ter regra: a tela não deixa chegar lá (para isso, remova a regra). */
export function recordsEverything(r: Recorded): boolean {
  return r.lifecycle && r.metrics && r.crashes;
}

/** Alterna um tipo mantendo a regra coerente. Devolve a mesma regra se a mudança não for permitida. */
export function toggle(rule: Rule, kind: ExclusionKind): Rule {
  const next: Recorded = { ...rule.recorded, [kind]: !rule.recorded[kind] };
  if (kind === "lifecycle" && !next.lifecycle) next.metrics = false; // sem início e fim, sem CPU e memória
  if (kind === "metrics" && next.metrics && !rule.recorded.lifecycle) return rule; // precisa do início e fim
  const fixed = coherent(next);
  if (recordsEverything(fixed)) return rule; // seria remover a regra: não por aqui
  return { exe: rule.exe, recorded: fixed };
}

/** Pode alternar este tipo agora? (usado para desabilitar caixas na tela) */
export function canToggle(rule: Rule, kind: ExclusionKind): boolean {
  return toggle(rule, kind) !== rule;
}

/** Regras mostradas a partir das configurações salvas: exclusões totais e parciais juntas, por nome. */
export function rulesFrom(s: Pick<Settings, "excludedApps" | "partialExclusions">): Rule[] {
  const rules = new Map<string, Rule>();
  for (const p of s.partialExclusions) {
    const excluded = new Set(p.kinds);
    const lifecycleOut = excluded.has("lifecycle");
    rules.set(p.exe, {
      exe: p.exe,
      recorded: coherent({ lifecycle: !lifecycleOut, metrics: !excluded.has("metrics") && !lifecycleOut, crashes: !excluded.has("crashes") }),
    });
  }
  // Uma exclusão total vence qualquer regra parcial do mesmo programa (o backend faz o mesmo).
  for (const exe of s.excludedApps) rules.set(exe, { exe, recorded: NOTHING });
  return [...rules.values()].sort((a, b) => a.exe.localeCompare(b.exe));
}

/** Converte as regras da tela de volta para o que o backend guarda. */
export function toSettings(rules: Rule[]): Pick<Settings, "excludedApps" | "partialExclusions"> {
  const excludedApps: string[] = [];
  const partialExclusions: PartialExclusion[] = [];
  for (const rule of [...rules].sort((a, b) => a.exe.localeCompare(b.exe))) {
    const r = coherent(rule.recorded);
    if (recordsEverything(r)) continue; // sem exclusão nenhuma: não há o que guardar
    if (!r.lifecycle && !r.metrics && !r.crashes) {
      excludedApps.push(rule.exe);
      continue;
    }
    const kinds = KINDS.filter((k) => !r[k]);
    partialExclusions.push({ exe: rule.exe, kinds });
  }
  return { excludedApps, partialExclusions };
}

/** Nova regra para um programa: exclui TUDO (o padrão seguro). */
export function newRule(exe: string): Rule {
  return { exe, recorded: NOTHING };
}
