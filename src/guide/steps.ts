import type { Key } from "../i18n";

/** Qual ilustração (inerte, com dados inventados) acompanha cada passo. */
export type DemoId = "welcome" | "light" | "paused" | "privacy" | "incidents" | "rules" | "extras" | "done";

export interface Step {
  id: DemoId;
  title: Key;
  /** Parágrafos do passo. */
  body: Key[];
}

/**
 * O tour da primeira abertura. Todo o texto vem dos dicionários (nos dois idiomas) e fica DENTRO do app: nada é
 * buscado na rede. Cada passo só explica e mostra um exemplo; nada aqui executa uma ação de verdade.
 */
export const TOUR: Step[] = [
  { id: "welcome", title: "guide.tour.welcome.title", body: ["guide.tour.welcome.b1", "guide.tour.welcome.b2"] },
  { id: "light", title: "guide.tour.light.title", body: ["guide.tour.light.b1", "guide.tour.light.b2"] },
  { id: "paused", title: "guide.tour.paused.title", body: ["guide.tour.paused.b1", "guide.tour.paused.b2"] },
  { id: "privacy", title: "guide.tour.privacy.title", body: ["guide.tour.privacy.b1", "guide.tour.privacy.b2"] },
  { id: "incidents", title: "guide.tour.incidents.title", body: ["guide.tour.incidents.b1", "guide.tour.incidents.b2"] },
  { id: "rules", title: "guide.tour.rules.title", body: ["guide.tour.rules.b1", "guide.tour.rules.b2"] },
  { id: "extras", title: "guide.tour.extras.title", body: ["guide.tour.extras.b1", "guide.tour.extras.b2"] },
  { id: "done", title: "guide.tour.done.title", body: ["guide.tour.done.b1"] },
];
