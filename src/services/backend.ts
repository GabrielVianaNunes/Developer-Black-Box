import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Status } from "../types/status";
import type { UpdateState } from "../types/update";
import type {
  ActivityFilter,
  ActivityRow,
  AppCandidate,
  Authorization,
  ConfigChange,
  Detection,
  ExportResult,
  Incident,
  IncidentDetail,
  Overview,
  ProcessRow,
  Settings,
  Storage,
  Verify,
} from "../types/dashboard";

// Versão instalada do app
export const getAppVersion = () => invoke<string>("get_app_version");

// Idioma da interface: o backend é a fonte da verdade (salva a escolha e retraduz a bandeja)
export const getLanguage = () => invoke<string>("get_language");
export const setLanguage = (language: string) => invoke<string>("set_language", { language });

// Estado da gravação
export const getStatus = () => invoke<Status>("get_status");
export const pauseRecording = () => invoke<Status>("pause_recording");
export const resumeRecording = () => invoke<Status>("resume_recording");

/** Atualizações emitidas pelo backend sempre que o estado real do recorder muda. */
export const onStatus = (cb: (s: Status) => void): Promise<UnlistenFn> =>
  listen<Status>("status", (e) => cb(e.payload));

export const onNavigate = (cb: (view: string) => void): Promise<UnlistenFn> =>
  listen<string>("navigate", (e) => cb(e.payload));

// Dashboard
export const getOverview = () => invoke<Overview>("get_overview");
export const getActivity = (filter: ActivityFilter) => invoke<ActivityRow[]>("get_activity", { filter });
export const getProcesses = () => invoke<ProcessRow[]>("get_processes");

// Incidentes
export const listIncidents = () => invoke<Incident[]>("list_incidents");
export const getIncident = (id: number) => invoke<IncidentDetail | null>("get_incident", { id });
export const setIncidentState = (id: number, state: string) => invoke<void>("set_incident_state", { id, state });
export const addIncidentNote = (id: number, text: string) => invoke<void>("add_incident_note", { id, text });
export const deleteIncident = (id: number) => invoke<void>("delete_incident", { id });
export const captureIncident = () => invoke<number>("capture_incident");
export const exportIncident = (id: number) => invoke<ExportResult>("export_incident", { id });

// Privacidade e armazenamento
export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) => invoke<Settings>("set_settings", { settings });
export const getConfigHistory = () => invoke<ConfigChange[]>("get_config_history");

// Início automático com o Windows
export const getLaunchAtLogin = () => invoke<boolean>("get_launch_at_login");
export const setLaunchAtLogin = (enabled: boolean) => invoke<boolean>("set_launch_at_login", { enabled });

// Modo de teste: autorizações temporárias
export const listAuthorizations = () => invoke<Authorization[]>("list_authorizations");
export const authorizeApp = (exe: string, minutes: number, allowMetrics: boolean, allowCrashes: boolean) =>
  invoke<void>("authorize_app", { exe, minutes, allowMetrics, allowCrashes });
export const revokeAuthorization = (exe: string) => invoke<boolean>("revoke_authorization", { exe });
export const getStorage = () => invoke<Storage>("get_storage");
export const verifyIntegrity = () => invoke<Verify>("verify_integrity");
export const deleteActivity = (includePreserved: boolean) => invoke<number>("delete_activity", { includePreserved });

// Atualizações: só avisa que existe versão nova; nada é baixado nem instalado
export const getUpdateState = () => invoke<UpdateState>("get_update_state");
export const setUpdateCheck = (enabled: boolean) => invoke<UpdateState>("set_update_check", { enabled });
export const checkForUpdates = () => invoke<UpdateState>("check_for_updates");
export const openReleasePage = () => invoke<void>("open_release_page");
export const onUpdate = (cb: (s: UpdateState) => void): Promise<UnlistenFn> =>
  listen<UpdateState>("update", (e) => cb(e.payload));
export const downloadUpdate = () => invoke<UpdateState>("download_update");
export const installUpdate = () => invoke<void>("install_update");

// Escolha de programas para as listas de privacidade (a lista é montada na hora e nunca é gravada)
/** Espera `delayMs` (para a pessoa trazer outro app para a frente) e devolve o NOME do executável em primeiro plano. */
export const detectForegroundApp = (delayMs: number) => invoke<Detection>("detect_foreground_app", { delayMs });
export const listAppCandidates = () => invoke<AppCandidate[]>("list_app_candidates");
export const pickExecutable = (title: string, filterLabel: string) =>
  invoke<string | null>("pick_executable", { title, filterLabel });

// Guia de boas-vindas: só duas informações (tour visto, versão em que um guia foi visto); o conteúdo vive no app
export interface GuideState {
  tourSeen: boolean;
  seenVersion: string | null;
  newsEnabled: boolean;
}
export const getGuideState = () => invoke<GuideState>("get_guide_state");
export const markTourSeen = () => invoke<void>("mark_tour_seen");
export const markNewsSeen = () => invoke<void>("mark_news_seen");
export const setNewsEnabled = (enabled: boolean) => invoke<GuideState>("set_news_enabled", { enabled });
