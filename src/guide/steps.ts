import type { Key } from "../i18n";

/** Qual ilustração (inerte, com dados inventados) acompanha cada passo. */
export type DemoId = "welcome" | "light" | "privacy" | "incidents" | "rules" | "extras" | "done" | "picker";

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
  { id: "welcome", title: "guide.tour.welcome.title", body: ["guide.tour.welcome.b1"] },
  { id: "light", title: "guide.tour.light.title", body: ["guide.tour.light.b1", "guide.tour.light.b2"] },
  { id: "privacy", title: "guide.tour.privacy.title", body: ["guide.tour.privacy.b1"] },
  { id: "incidents", title: "guide.tour.incidents.title", body: ["guide.tour.incidents.b1"] },
  { id: "rules", title: "guide.tour.rules.title", body: ["guide.tour.rules.b1"] },
  { id: "extras", title: "guide.tour.extras.title", body: ["guide.tour.extras.b1", "guide.tour.extras.b2"] },
];
