//! Estado do guia de boas-vindas: só duas coisas são guardadas, e nenhuma é pessoal.
//!
//! * `guide_tour_seen`: o tour da primeira abertura já foi visto (ou pulado, ou dispensado porque o usuário já
//!   conhecia o app).
//! * `guide_seen_version`: a versão do app em que o usuário viu um guia pela última vez (usada pelas novidades
//!   de atualização, na fase seguinte).
//!
//! * `guide_news_off`: a pessoa pediu para não ver o resumo de novidades depois de atualizar.
//!
//! Ficam no mesmo banco cifrado das demais configurações. O conteúdo do guia está DENTRO do app: nada é buscado
//! na rede e este módulo não executa nenhuma ação do usuário (não grava, não muda regra, não liga nada).

use std::sync::Arc;

use bb_store::Store;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::Runtime;

const TOUR_SEEN: &str = "guide_tour_seen";
const SEEN_VERSION: &str = "guide_seen_version";
/// Só existe (com "true") quando a pessoa desligou as novidades pós-atualização: por padrão elas aparecem.
const NEWS_OFF: &str = "guide_news_off";

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GuideState {
    /// Já pode deixar de mostrar o tour sozinho (visto, pulado ou desnecessário).
    pub tour_seen: bool,
    /// Versão do app em que um guia foi visto pela última vez, se houver.
    pub seen_version: Option<String>,
    /// O resumo de novidades depois de uma atualização está ligado (padrão) ou a pessoa o desligou.
    pub news_enabled: bool,
}

/// O tour é mostrado sozinho só a quem NÃO viu e é um usuário novo. Quem já usava o app antes do guia existir
/// (tem histórico de configuração, incidentes ou idioma escolhido) já conhece o programa: para essa pessoa o tour
/// completo só aparece se ela abrir pelo botão de ajuda.
pub fn tour_should_open(already_seen: bool, existing_user: bool) -> bool {
    !already_seen && !existing_user
}

/// Sinais de que a pessoa já usou o app (nenhum é conteúdo: só "existe ou não").
fn existing_user(store: &Store) -> Result<bool, String> {
    let history = !store.config_history(1).map_err(|_| "store.error".to_string())?.is_empty();
    let incidents = !store.list_incidents().map_err(|_| "store.error".to_string())?.is_empty();
    let language = store.get_setting("language").ok().flatten().is_some();
    Ok(history || incidents || language)
}

/// Estado do guia. Um usuário que já conhecia o app é marcado como "tour visto" na primeira consulta, para o
/// tour não aparecer sozinho para ele (ele ainda pode abri-lo pelo botão "?").
pub fn state_of(store: &Store) -> Result<GuideState, String> {
    let seen = store.get_setting(TOUR_SEEN).ok().flatten().is_some_and(|v| v == "true");
    let seen_version = store.get_setting(SEEN_VERSION).ok().flatten();
    let mut tour_seen = seen;
    if !seen && !tour_should_open(seen, existing_user(store)?) {
        store.set_setting(TOUR_SEEN, "true").map_err(|_| "store.error".to_string())?;
        tour_seen = true;
    }
    Ok(GuideState { tour_seen, seen_version, news_enabled: news_enabled(store) })
}

fn news_enabled(store: &Store) -> bool {
    // Falha de leitura não desliga nada por engano nem liga o que a pessoa desligou: sem valor legível, vale o padrão.
    !store.get_setting(NEWS_OFF).ok().flatten().is_some_and(|v| v == "true")
}

/// Liga ou desliga o resumo de novidades. O histórico guarda só "guide_news: changed", nunca o conteúdo.
pub fn set_news(store: &Store, enabled: bool, now_utc_ms: i64) -> Result<(), String> {
    let was = news_enabled(store);
    store.set_setting(NEWS_OFF, if enabled { "false" } else { "true" }).map_err(|_| "store.error".to_string())?;
    if was != enabled {
        let _ = store.log_config_change(now_utc_ms, "guide_news", "changed");
    }
    Ok(())
}

/// O resumo de novidades foi visto ou pulado: guarda a versão atual (não mexe no tour da primeira abertura).
pub fn record_news_seen(store: &Store, version: &str) -> Result<(), String> {
    store.set_setting(SEEN_VERSION, version).map_err(|_| "store.error".to_string())
}

/// O usuário terminou ou pulou o tour: não mostrar sozinho de novo. Guarda também a versão atual.
pub fn mark_seen(store: &Store, version: &str) -> Result<(), String> {
    store.set_setting(TOUR_SEEN, "true").map_err(|_| "store.error".to_string())?;
    store.set_setting(SEEN_VERSION, version).map_err(|_| "store.error".to_string())
}

fn with_store<T>(app: &AppHandle, f: impl FnOnce(&Store) -> Result<T, String>) -> Result<T, String> {
    let rt = app.state::<Arc<Runtime>>();
    let engine = rt.engine.lock().map_err(|_| "internal".to_string())?;
    let Some(store) = engine.store() else { return Err("store.unavailable".into()) };
    f(store)
}

#[tauri::command]
pub fn get_guide_state(app: AppHandle) -> Result<GuideState, String> {
    with_store(&app, state_of)
}

#[tauri::command]
pub fn mark_tour_seen(app: AppHandle) -> Result<(), String> {
    with_store(&app, |s| mark_seen(s, env!("CARGO_PKG_VERSION")))
}

#[tauri::command]
pub fn mark_news_seen(app: AppHandle) -> Result<(), String> {
    with_store(&app, |s| record_news_seen(s, env!("CARGO_PKG_VERSION")))
}

#[tauri::command]
pub fn set_news_enabled(app: AppHandle, enabled: bool) -> Result<GuideState, String> {
    with_store(&app, |s| {
        set_news(s, enabled, crate::utc_ms())?;
        state_of(s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Store::open_encrypted(dir.path().join("meta.db"), &[3u8; 32]).unwrap(), dir)
    }

    #[test]
    fn the_tour_opens_by_itself_only_for_new_users_who_have_not_seen_it() {
        assert!(tour_should_open(false, false), "a fresh install sees the tour");
        assert!(!tour_should_open(true, false), "already seen or skipped");
        assert!(!tour_should_open(false, true), "someone who already used the app does not get a forced tour");
        assert!(!tour_should_open(true, true));
    }

    #[test]
    fn a_fresh_database_is_a_new_user_and_stays_pending_until_marked() {
        let (s, _d) = store();
        assert_eq!(state_of(&s).unwrap(), GuideState { tour_seen: false, seen_version: None, news_enabled: true });
        assert_eq!(state_of(&s).unwrap().tour_seen, false, "asking does not mark a new user");
        mark_seen(&s, "9.9.9").unwrap();
        assert_eq!(state_of(&s).unwrap(), GuideState { tour_seen: true, seen_version: Some("9.9.9".into()), news_enabled: true });
    }

    #[test]
    fn people_who_already_used_the_app_are_not_forced_through_the_tour() {
        // Qualquer um destes sinais conta: histórico de configuração ou idioma escolhido.
        let (s, _d) = store();
        s.log_config_change(1, "auto_start", "changed").unwrap();
        assert_eq!(state_of(&s).unwrap().tour_seen, true);
        assert_eq!(s.get_setting(TOUR_SEEN).unwrap().as_deref(), Some("true"), "decided once and remembered");
        assert_eq!(s.get_setting(SEEN_VERSION).unwrap(), None, "no guide was shown, so no version is recorded");

        let (s2, _d2) = store();
        s2.set_setting("language", "pt-BR").unwrap();
        assert_eq!(state_of(&s2).unwrap().tour_seen, true);
    }

    #[test]
    fn the_stored_values_are_only_flags_and_a_version_and_are_encrypted_with_the_other_settings() {
        let (s, d) = store();
        mark_seen(&s, "0.3.0").unwrap();
        let db = std::fs::read(d.path().join("meta.db")).unwrap();
        for plain in ["guide_tour_seen", "guide_seen_version", "0.3.0"] {
            // O nome da chave pode aparecer no esquema do banco, mas o VALOR guardado é cifrado como os demais.
            if plain == "0.3.0" {
                assert!(!db.windows(plain.len()).any(|w| w == plain.as_bytes()), "value found in clear on disk");
            }
        }
        assert_eq!(s.get_setting(SEEN_VERSION).unwrap().as_deref(), Some("0.3.0"));
    }

    #[test]
    fn news_are_on_by_default_and_the_choice_to_turn_them_off_is_remembered_and_logged() {
        let (s, _d) = store();
        assert!(state_of(&s).unwrap().news_enabled, "on by default");
        set_news(&s, false, 5).unwrap();
        assert!(!state_of(&s).unwrap().news_enabled);
        set_news(&s, false, 6).unwrap();
        set_news(&s, true, 7).unwrap();
        assert!(state_of(&s).unwrap().news_enabled);
        let log = s.config_history(10).unwrap();
        assert_eq!(log.len(), 2, "only real changes are logged (off, then on)");
        assert!(log.iter().all(|c| c.key == "guide_news" && c.change == "changed"), "key and kind only");
    }

    #[test]
    fn seeing_the_news_records_the_version_without_touching_the_first_run_tour() {
        let (s, _d) = store();
        record_news_seen(&s, "9.9.9").unwrap();
        let st = state_of(&s).unwrap();
        assert_eq!(st.seen_version.as_deref(), Some("9.9.9"));
        assert!(!st.tour_seen, "a new user still gets the tour");
    }
}
