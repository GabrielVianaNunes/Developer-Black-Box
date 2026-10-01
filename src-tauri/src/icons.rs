//! Renovação do cache de ícones do Windows, uma vez por versão do app.
//!
//! O Windows guarda em cache o ícone do executável (botão da barra de tarefas, atalhos, busca) e, ao atualizar de uma
//! versão que tinha outro ícone, continua mostrando o antigo (por exemplo, o cubo com a luz cinza ao lado). Na primeira
//! abertura de cada versão o app pede ao Windows, com a notificação oficial de "os ícones mudaram", que releia os
//! ícones. Isso acontece ANTES de qualquer janela existir, para o botão já nascer com o ícone certo. Nada é alterado em
//! arquivo ou configuração além de uma marca com a versão (cifrada junto das demais configurações).

use bb_store::Store;

const MARK: &str = "icons_refreshed_version";

/// Pede a renovação só quando ainda não foi pedida para esta versão.
pub fn should_refresh(saved: Option<&str>, current: &str) -> bool {
    saved != Some(current)
}

/// Pede a renovação (uma vez por versão) e grava a marca. `notify` é a chamada ao Windows (trocável nos testes).
/// Se a marca não puder ser lida ou gravada, o resultado é pedir de novo na próxima abertura: inofensivo.
pub fn refresh_once(store: &Store, current: &str, notify: impl FnOnce()) -> bool {
    let saved = store.get_setting(MARK).ok().flatten();
    if !should_refresh(saved.as_deref(), current) {
        return false;
    }
    notify();
    let _ = store.set_setting(MARK, current);
    true
}

/// SHChangeNotify(SHCNE_ASSOCCHANGED): o aviso oficial de que ícones e associações mudaram.
pub fn notify_windows() {
    use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_FLUSH, SHCNF_IDLIST};
    // SAFETY: a notificação global não usa os ponteiros (ambos nulos).
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST | SHCNF_FLUSH, None, None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (Store::open_encrypted(dir.path().join("meta.db"), &[5u8; 32]).unwrap(), dir)
    }

    #[test]
    fn it_asks_only_when_this_version_has_not_asked_yet() {
        assert!(should_refresh(None, "0.3.5"), "first run ever");
        assert!(should_refresh(Some("0.3.4"), "0.3.5"), "after an update");
        assert!(!should_refresh(Some("0.3.5"), "0.3.5"), "already done for this version");
    }

    #[test]
    fn the_notification_is_sent_once_per_version_and_remembered() {
        let (s, _d) = store();
        let calls = Cell::new(0);
        assert!(refresh_once(&s, "0.3.5", || calls.set(calls.get() + 1)));
        assert!(!refresh_once(&s, "0.3.5", || calls.set(calls.get() + 1)), "not again on the next start");
        assert_eq!(calls.get(), 1);
        assert!(refresh_once(&s, "0.3.6", || calls.set(calls.get() + 1)), "a new version asks again");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn the_mark_is_just_the_version_and_is_encrypted_like_the_other_settings() {
        let (s, d) = store();
        refresh_once(&s, "0.3.5", || {});
        assert_eq!(s.get_setting(MARK).unwrap().as_deref(), Some("0.3.5"));
        let db = std::fs::read(d.path().join("meta.db")).unwrap();
        assert!(!db.windows(5).any(|w| w == b"0.3.5"), "the version must not appear in clear on disk");
    }
}
