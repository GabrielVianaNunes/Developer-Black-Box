//! Configurações editáveis pelo usuário: validadas, aplicadas na hora e persistidas.
//!
//! O histórico de mudanças guarda só QUAL chave mudou e o tipo da mudança
//! ("added", "removed", "changed"), nunca o valor, que poderia ser o nome de um app.

use std::collections::BTreeSet;

use bb_core::{ExclusionSet, ExeName, GuardConfig};
use bb_store::Store;

/// Exclusão parcial: deste programa, só os tipos de evento em `excluded` deixam de ser gravados.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialExclusion {
    pub exe: String,
    pub excluded: ExclusionSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub protected_apps: Vec<String>,
    pub excluded_apps: Vec<String>,
    /// Programas excluídos só em alguns tipos de evento (os outros continuam gravados). Uma regra que exclui
    /// todos os tipos é guardada como exclusão total em `excluded_apps`.
    pub partial_exclusions: Vec<PartialExclusion>,
    /// Programas de `excluded_apps` cuja exclusão vale também para os processos que eles iniciam (filhos, netos...).
    /// Só faz sentido para exclusão total: nomes que não estão em `excluded_apps` são descartados ao normalizar.
    pub excluded_trees: Vec<String>,
    pub stability_window_ms: u64,
    /// Começa a gravar ao abrir o app. Padrão `false`: só grava depois de você autorizar.
    pub auto_start: bool,
    pub retention_max_mb: u64,
    pub retention_max_hours: u64,
    /// Telemetria contínua de desempenho do sistema (contadores PDH, ~1 amostra a cada 30 s). Ligada por padrão; é
    /// a chave para desligar na aba Privacidade.
    pub telemetry_enabled: bool,
    /// Leitura dos eventos de saúde do Windows (Event Log). Ligada por padrão; desligada, nada é lido nem gravado e o
    /// período desligado nunca é lido depois.
    pub health_log_enabled: bool,
    /// Leitura do inventário da máquina (BIOS, firmware, Secure Boot, build, drivers). Mesmas regras.
    pub inventory_enabled: bool,
    /// Leitura de energia e bateria. Mesmas regras.
    pub power_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let g = GuardConfig::default();
        let mut protected: Vec<String> = g.protected_apps.iter().map(|n| n.as_str().to_owned()).collect();
        protected.sort();
        Settings {
            protected_apps: protected,
            excluded_apps: Vec::new(),
            partial_exclusions: Vec::new(),
            excluded_trees: Vec::new(),
            stability_window_ms: g.stability_window_ms,
            auto_start: false,
            retention_max_mb: 256,
            retention_max_hours: 24,
            telemetry_enabled: true,
            health_log_enabled: true,
            inventory_enabled: true,
            power_enabled: true,
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
        if self.partial_exclusions.len() > MAX_APPS || self.excluded_trees.len() > MAX_APPS {
            return Err("settings.too_many_excluded".into());
        }
        for n in &self.excluded_trees {
            ExeName::new(n).map_err(|_| "settings.bad_excluded_name".to_string())?;
        }
        for rule in &self.partial_exclusions {
            ExeName::new(&rule.exe).map_err(|_| "settings.bad_excluded_name".to_string())?;
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
            partial_exclusions: self
                .partial_exclusions
                .iter()
                .filter_map(|r| ExeName::new(&r.exe).ok().map(|n| (n, r.excluded)))
                .collect(),
            excluded_trees: names(&self.excluded_trees),
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

        // Exclusões parciais: nomes em minúsculas, uma regra por programa (duas viram a UNIÃO: o lado mais
        // privado), regras vazias somem, regra que exclui tudo vira exclusão total, e quem já está excluído
        // por inteiro não precisa de regra parcial.
        let mut merged: std::collections::BTreeMap<String, ExclusionSet> = std::collections::BTreeMap::new();
        for r in std::mem::take(&mut self.partial_exclusions) {
            let exe = r.exe.trim().to_lowercase();
            if exe.is_empty() {
                continue;
            }
            let e = merged.entry(exe).or_insert(ExclusionSet::NONE);
            *e = e.union(r.excluded);
        }
        let mut full: BTreeSet<String> = self.excluded_apps.iter().cloned().collect();
        for (exe, set) in merged {
            if set.is_empty() || full.contains(&exe) {
                continue;
            }
            if set.is_all() {
                full.insert(exe);
            } else {
                self.partial_exclusions.push(PartialExclusion { exe, excluded: set });
            }
        }
        self.excluded_apps = full.into_iter().collect();
        // A opção dos filhos só vale para quem está excluído POR INTEIRO.
        let all_out: BTreeSet<String> = self.excluded_apps.iter().cloned().collect();
        self.excluded_trees = norm(std::mem::take(&mut self.excluded_trees)).into_iter().filter(|t| all_out.contains(t)).collect();
        self
    }

    pub fn load(store: &Store) -> Self {
        let d = Settings::default();
        let list = |k: &str, dflt: &Vec<String>| match store.get_setting(k) {
            Ok(Some(v)) => v.lines().map(str::to_owned).filter(|s| !s.is_empty()).collect(),
            _ => dflt.clone(),
        };
        let switch = |k: &str, dflt: bool| match store.get_setting(k) {
            Ok(None) => dflt,
            Ok(Some(v)) => v == "true",
            Err(_) => false,
        };
        let num = |k: &str, dflt: u64| store.get_setting(k).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(dflt);
        let s = Settings {
            protected_apps: list("protected_apps", &d.protected_apps),
            excluded_apps: list("excluded_apps", &d.excluded_apps),
            partial_exclusions: load_partial(store),
            excluded_trees: list("excluded_trees", &d.excluded_trees),
            stability_window_ms: num("stability_window_ms", d.stability_window_ms),
            auto_start: store.get_setting("auto_start").ok().flatten().map_or(d.auto_start, |v| v == "true"),
            retention_max_mb: num("retention_max_mb", d.retention_max_mb),
            retention_max_hours: num("retention_max_hours", d.retention_max_hours),
            // Interruptores: ausente = padrão (ligado). Valor ilegível ou diferente de "true" = DESLIGADO: na dúvida, não coleta.
            telemetry_enabled: switch("telemetry_enabled", d.telemetry_enabled),
            health_log_enabled: switch("health_log_enabled", d.health_log_enabled),
            inventory_enabled: switch("inventory_enabled", d.inventory_enabled),
            power_enabled: switch("power_enabled", d.power_enabled),
        };
        // Valores fora do intervalo (banco editado à mão) voltam ao padrão: fail-closed para o Guard.
        let s = s.normalized();
        if s.validate().is_ok() { s } else { d }
    }

    pub fn save(&self, store: &Store) -> Result<(), bb_store::StoreError> {
        store.set_setting("protected_apps", &self.protected_apps.join("\n"))?;
        store.set_setting("excluded_apps", &self.excluded_apps.join("\n"))?;
        store.set_setting("partial_exclusions", &encode_partial(&self.partial_exclusions))?;
        store.set_setting("excluded_trees", &self.excluded_trees.join("\n"))?;
        store.set_setting("stability_window_ms", &self.stability_window_ms.to_string())?;
        store.set_setting("auto_start", if self.auto_start { "true" } else { "false" })?;
        store.set_setting("retention_max_mb", &self.retention_max_mb.to_string())?;
        for (k, on) in [
            ("telemetry_enabled", self.telemetry_enabled),
            ("health_log_enabled", self.health_log_enabled),
            ("inventory_enabled", self.inventory_enabled),
            ("power_enabled", self.power_enabled),
        ] {
            store.set_setting(k, if on { "true" } else { "false" })?;
        }
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
        list_change("excluded_trees", &self.excluded_trees, &new.excluded_trees, &mut out);
        // Exclusões parciais: só a CHAVE e o tipo da mudança vão para o histórico, nunca os nomes nem os tipos.
        let (old_map, new_map): (std::collections::BTreeMap<_, _>, std::collections::BTreeMap<_, _>) = (
            self.partial_exclusions.iter().map(|r| (&r.exe, r.excluded)).collect(),
            new.partial_exclusions.iter().map(|r| (&r.exe, r.excluded)).collect(),
        );
        let added = new_map.keys().any(|k| !old_map.contains_key(k));
        let removed = old_map.keys().any(|k| !new_map.contains_key(k));
        let altered = new_map.iter().any(|(k, v)| old_map.get(k).is_some_and(|o| o != v));
        match (added, removed, altered) {
            (false, false, false) => {}
            (true, false, false) => out.push(("partial_exclusions", "added")),
            (false, true, false) => out.push(("partial_exclusions", "removed")),
            _ => out.push(("partial_exclusions", "changed")),
        }
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
        if self.telemetry_enabled != new.telemetry_enabled {
            out.push(("telemetry_enabled", "changed"));
        }
        if self.health_log_enabled != new.health_log_enabled {
            out.push(("health_log_enabled", "changed"));
        }
        if self.inventory_enabled != new.inventory_enabled {
            out.push(("inventory_enabled", "changed"));
        }
        if self.power_enabled != new.power_enabled {
            out.push(("power_enabled", "changed"));
        }
        out
    }
}

/// `exe|codigo,codigo` por linha. Linha ilegível: o programa vira exclusão TOTAL (falha fechada), nunca é ignorado.
fn encode_partial(rules: &[PartialExclusion]) -> String {
    rules.iter().map(|r| format!("{}|{}", r.exe, r.excluded.to_codes())).collect::<Vec<_>>().join("\n")
}

fn load_partial(store: &Store) -> Vec<PartialExclusion> {
    let Ok(Some(text)) = store.get_setting("partial_exclusions") else { return Vec::new() };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (exe, codes) = l.split_once('|').unwrap_or((l, ""));
            PartialExclusion { exe: exe.to_owned(), excluded: ExclusionSet::from_codes(codes) }
        })
        .collect()
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

    #[test]
    fn the_children_option_only_survives_for_a_full_exclusion_and_is_normalized() {
        let mut s = Settings::default();
        s.excluded_apps = vec!["Synth-Root.EXE".into()];
        s.excluded_trees = vec!["synth-root.exe".into(), "SYNTH-ROOT.exe".into(), "never-excluded.exe".into(), " ".into()];
        s.partial_exclusions = vec![PartialExclusion { exe: "synth-partial.exe".into(), excluded: ExclusionSet::new(false, true, false) }];
        let n = s.normalized();
        assert_eq!(n.excluded_trees, vec!["synth-root.exe".to_string()], "lowercase, no repeats, only fully excluded programs");
        let mut p = Settings::default();
        p.partial_exclusions = vec![PartialExclusion { exe: "synth-partial.exe".into(), excluded: ExclusionSet::new(false, true, false) }];
        p.excluded_trees = vec!["synth-partial.exe".into()];
        assert!(p.normalized().excluded_trees.is_empty(), "a partial rule is never extended to the tree");
    }

    #[test]
    fn a_rule_that_stops_excluding_the_whole_program_loses_the_children_option() {
        let mut s = Settings::default();
        s.excluded_apps = vec![];
        s.excluded_trees = vec!["synth-root.exe".into()];
        assert!(s.normalized().excluded_trees.is_empty());
    }

    #[test]
    fn the_children_option_reaches_the_guard_and_changes_are_logged_without_names() {
        let mut s = Settings::default();
        s.excluded_apps = vec!["synth-root.exe".into()];
        s.excluded_trees = vec!["synth-root.exe".into()];
        let g = s.normalized().guard_config();
        assert!(g.excluded_trees.contains(&ExeName::new("synth-root.exe").unwrap()));
        let before = Settings::default();
        let mut after = before.clone();
        after.excluded_apps = vec!["synth-root.exe".into()];
        after.excluded_trees = vec!["synth-root.exe".into()];
        let diff = before.diff(&after);
        assert!(diff.contains(&("excluded_trees", "added")), "{diff:?}");
        assert!(diff.iter().all(|(k, c)| !k.contains("synth") && !c.contains("synth")), "never values");
    }

    #[test]
    fn telemetry_is_on_by_default_and_the_switch_round_trips() {
        assert!(Settings::default().telemetry_enabled);
        let store = Store::open_in_memory().unwrap();
        assert!(Settings::load(&store).telemetry_enabled, "nothing saved yet = default (on)");
        let mut off = Settings::default();
        off.telemetry_enabled = false;
        off.save(&store).unwrap();
        assert!(!Settings::load(&store).telemetry_enabled, "an explicit off is respected");
        let mut on = off.clone();
        on.telemetry_enabled = true;
        on.save(&store).unwrap();
        assert!(Settings::load(&store).telemetry_enabled);
    }

    #[test]
    fn an_unreadable_telemetry_value_means_off_not_on() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("telemetry_enabled", "maybe").unwrap();
        assert!(!Settings::load(&store).telemetry_enabled, "when in doubt, do not collect");
    }

    #[test]
    fn the_telemetry_switch_is_logged_as_a_key_change_without_a_value() {
        let before = Settings::default();
        let mut after = before.clone();
        after.telemetry_enabled = false;
        assert_eq!(before.diff(&after), vec![("telemetry_enabled", "changed")]);
        assert!(before.diff(&before).is_empty());
    }

    #[test]
    fn the_health_source_switches_default_on_round_trip_and_unclear_means_off() {
        let store = Store::open_in_memory().unwrap();
        let d = Settings::default();
        assert!(d.health_log_enabled && d.inventory_enabled && d.power_enabled);
        let l = Settings::load(&store);
        assert!(l.health_log_enabled && l.inventory_enabled && l.power_enabled, "nothing saved yet = default (on)");
        let mut s = d.clone();
        s.health_log_enabled = false;
        s.power_enabled = false;
        s.save(&store).unwrap();
        let l = Settings::load(&store);
        assert!(!l.health_log_enabled && l.inventory_enabled && !l.power_enabled, "each one is stored on its own");
        for key in ["health_log_enabled", "inventory_enabled", "power_enabled"] {
            store.set_setting(key, "maybe").unwrap();
        }
        let l = Settings::load(&store);
        assert!(!l.health_log_enabled && !l.inventory_enabled && !l.power_enabled, "when in doubt, do not collect");
        let mut after = d.clone();
        after.inventory_enabled = false;
        assert_eq!(d.diff(&after), vec![("inventory_enabled", "changed")]);
        after.health_log_enabled = false;
        after.power_enabled = false;
        assert_eq!(d.diff(&after).len(), 3);
    }

    #[test]
    fn a_telemetry_value_that_cannot_be_decrypted_means_off() {
        let dir = std::env::temp_dir().join(format!("bb-telemetry-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("meta.db");
        {
            let store = Store::open_encrypted(&db, &[1u8; 32]).unwrap();
            store.set_setting("telemetry_enabled", "true").unwrap();
            assert!(Settings::load(&store).telemetry_enabled, "positive control: readable with the right key");
        }
        let wrong = Store::open_encrypted(&db, &[2u8; 32]).unwrap();
        assert!(wrong.get_setting("telemetry_enabled").is_err(), "setup: the value really cannot be read");
        assert!(!Settings::load(&wrong).telemetry_enabled, "unreadable means do not collect");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
