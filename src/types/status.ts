export type Light = "green" | "red" | "gray";

/** Espelha `StatusDto` do backend. Nunca contém nomes de aplicativos. */
export interface Status {
  state: string;
  reason: string;
  text: string;
  light: Light;
  manuallyPaused: boolean;
  canPause: boolean;
  canResume: boolean;
}
