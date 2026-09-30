//! Configurações editáveis pelo usuário: validadas, aplicadas na hora e persistidas.
//!
//! O histórico de mudanças guarda só QUAL chave mudou e o tipo da mudança
//! ("added", "removed", "changed"), nunca o valor, que poderia ser o nome de um app.

use std::collections::BTreeSet;

use bb_core::{ExeName, GuardConfig};
use bb_store::Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub protected_apps: Vec<String>,
    pub excluded_apps: Vec<String>,
    pub stability_window_ms: u64,
    /// Começa a gravar ao abrir o app. Padrão `false`: só grava depois de você autorizar.
    pub auto_start: bool,
    pub retention_max_mb: u64,
    pub retention_max_hours: u64,
}

impl Default for Settings {
    fn default() -> Self {
        let g = GuardConfig::default();
        let mut protected: Vec<String> = g.protected_apps.iter().map(|n| n.as_str().to_owned()).collect();
        protected.sort();
        Settings {
            protected_apps: protected,
            excluded_apps: Vec::new(),
            stability_window_ms: g.stability_window_ms,
            auto_start: false,
            retention_max_mb: 256,
            retention_max_hours: 24,
        }
    }
}

const MAX_APPS: usize = 200;

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        // Os erros são CÓDIGOS (a interface os traduz), nunca texto de um idioma.
        for (list, too_many, bad_name) in [
            (&self.protected_apps, "settings.too_many_protected", "settings.bad_protected_name"),
            (&self.excluded_apps, "settings.too_many_excluded", "settings.bad_excluded_name"),
        ] {
            if list.len() > MAX_APPS {
                return Err(too_many.into());
            }
            for n in list {
                ExeName::new(n).map_err(|_| bad_name.to_string())?;
            }
        }
        if !(1_000..=60_000).contains(&self.stability_window_ms) {
            return Err("settings.stability_range".into());
        }
        if !(16..=10_240).contains(&self.retention_max_mb) {
            return Err("settings.storage_range".into());
        }
        if !(1..=720).contains(&self.retention_max_hours) {
            return Err("settings.retention_range".into());
        }
        Ok(())
    }

    /// Configuração do Guard. Pressupõe `validate()` já feito.
    pub fn guard_config(&self) -> GuardConfig {
        let names = |v: &Vec<String>| v.iter().filter_map(|n| ExeName::new(n).ok()).collect();
        GuardConfig {
            protected_apps: names(&self.protected_apps),
            excluded_apps: names(&self.excluded_apps),
            stability_window_ms: self.stability_window_ms,
            max_staleness_ms: GuardConfig::default().max_staleness_ms,
        }
    }

    /// Normaliza: nomes em minúsculas, sem repetidos, ordenados.
    pub fn normalized(mut self) -> Self {
        let norm = |v: Vec<String>| -> Vec<String> {
            v.into_iter().map(|s| s.trim().to_lowercase()).filter(|s| !s.is_empty()).collect::<BTreeSet<_>>().into_iter().collect()
        };
        self.protected_apps = norm(self.protected_apps);
        self.excluded_apps = norm(self.excluded_apps);
        self
    }

    pub fn load(store: &Store) -> Self {
        let d = Settings::default();
        let list = |k: &str, dflt: &Vec<String>| match store.get_setting(k) {
            Ok(Some(v)) => v.lines().map(str::to_owned).filter(|s| !s.is_empty()).collect(),
            _ => dflt.clone(),
        };
        let num = |k: &str, dflt: u64| store.get_setting(k).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(dflt);
        let s = Settings {
            protected_apps: list("protected_apps", &d.protected_apps),
            excluded_apps: list("excluded_apps", &d.excluded_apps),
            stability_window_ms: num("stability_window_ms", d.stability_window_ms),
            auto_start: store.get_setting("auto_start").ok().flatten().map_or(d.auto_start, |v| v == "true"),
            retention_max_mb: num("retention_max_mb", d.retention_max_mb),
            retention_max_hours: num("retention_max_hours", d.retention_max_hours),
        };
        // Valores fora do intervalo (banco editado à mão) voltam ao padrão: fail-closed para o Guard.
        if s.validate().is_ok() { s } else { d }
    }

    pub fn save(&self, store: &Store) -> Result<(), bb_store::StoreError> {
        store.set_setting("protected_apps", &self.protected_apps.join("\n"))?;
        store.set_setting("excluded_apps", &self.excluded_apps.join("\n"))?;
        store.set_setting("stability_window_ms", &self.stability_window_ms.to_string())?;
        store.set_setting("auto_start", if self.auto_start { "true" } else { "false" })?;
        store.set_setting("retention_max_mb", &self.retention_max_mb.to_string())?;
        store.set_setting("retention_max_hours", &self.retention_max_hours.to_string())
    }

    /// (chave, tipo da mudança) para cada campo alterado. Nunca inclui valores.
    pub fn diff(&self, new: &Settings) -> Vec<(&'static str, &'static str)> {
        let mut out = Vec::new();
        let list_change = |key: &'static str, a: &Vec<String>, b: &Vec<String>, out: &mut Vec<(&'static str, &'static str)>| {
            let (sa, sb): (BTreeSet<_>, BTreeSet<_>) = (a.iter().collect(), b.iter().collect());
            let added = sb.difference(&sa).next().is_some();
            let removed = sa.difference(&sb).next().is_some();
            match (added, removed) {
                (true, true) => out.push((key, "changed")),
                (true, false) => out.push((key, "added")),
                (false, true) => out.push((key, "removed")),
                _ => {}
            }
        };
        list_change("protected_apps", &self.protected_apps, &new.protected_apps, &mut out);
        list_change("excluded_apps", &self.excluded_apps, &new.excluded_apps, &mut out);
        if self.stability_window_ms != new.stability_window_ms {
            out.push(("stability_window_ms", "changed"));
        }
        if self.auto_start != new.auto_start {
            out.push(("auto_start", "changed"));
        }
        if self.retention_max_mb != new.retention_max_mb {
            out.push(("retention_max_mb", "changed"));
        }
        if self.retention_max_hours != new.retention_max_hours {
            out.push(("retention_max_hours", "changed"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_conservative() {
        let s = Settings::default();
        assert!(s.validate().is_ok());
        assert!(!s.auto_start, "recording must not start without the user's authorization");
        assert!(s.protected_apps.iter().any(|a| a == "chrome.exe"));
    }

    #[test]
    fn validation_rejects_paths_and_out_of_range_values() {
        let mut s = Settings::default();
        s.excluded_apps = vec!["C:\\x\\a.exe".into()];
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.stability_window_ms = 10;
        assert!(s.validate().is_err());
        let mut s = Settings::default();
        s.retention_max_mb = 1;
        assert!(s.validate().is_err());
    }

    #[test]
    fn validation_errors_are_language_neutral_codes() {
        let mut s = Settings::default();
        s.excluded_apps = vec!["C:\\x\\a.exe".into()];
        assert_eq!(s.validate(), Err("settings.bad_excluded_name".to_string()));
        let mut s = Settings::default();
        s.protected_apps = vec!["bad/name.exe".into()];
        assert_eq!(s.validate(), Err("settings.bad_protected_name".to_string()));
        let mut s = Settings::default();
        s.stability_window_ms = 5;
        assert_eq!(s.validate(), Err("settings.stability_range".to_string()));
        let mut s = Settings::default();
        s.retention_max_mb = 1;
        assert_eq!(s.validate(), Err("settings.storage_range".to_string()));
        let mut s = Settings::default();
        s.retention_max_hours = 0;
        assert_eq!(s.validate(), Err("settings.retention_range".to_string()));
        let mut s = Settings::default();
        s.excluded_apps = (0..201).map(|i| format!("a{i}.exe")).collect();
        assert_eq!(s.validate(), Err("settings.too_many_excluded".to_string()));
    }

    #[test]
    fn normalization_lowercases_trims_and_dedups() {
        let mut s = Settings::default();
        s.excluded_apps = vec![" Foo.EXE ".into(), "foo.exe".into(), "".into()];
        assert_eq!(s.normalized().excluded_apps, vec!["foo.exe"]);
    }

    #[test]
    fn diff_reports_kinds_of_change_never_values() {
        let a = Settings::default();
        let mut b = a.clone();
        b.excluded_apps = vec!["secret-app.exe".into()];
        b.auto_start = true;
        let d = a.diff(&b);
        assert_eq!(d, vec![("excluded_apps", "added"), ("auto_start", "changed")]);
        assert!(!format!("{d:?}").contains("secret-app"));
    }

    #[test]
    fn save_and_load_round_trip_and_bad_data_falls_back_to_defaults() {
        let store = Store::open_in_memory().unwrap();
        let mut s = Settings::default();
        s.excluded_apps = vec!["foo.exe".into()];
        s.stability_window_ms = 8_000;
        s.save(&store).unwrap();
        assert_eq!(Settings::load(&store), s);
        store.set_setting("stability_window_ms", "1").unwrap();
        assert_eq!(Settings::load(&store), Settings::default());
    }
}
