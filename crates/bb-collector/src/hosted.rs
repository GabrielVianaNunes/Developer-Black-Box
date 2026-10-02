//! Apps UWP e o hospedeiro de janelas do Windows (`ApplicationFrameHost.exe`).
//!
//! Para um app UWP (Calculadora, Configurações, Fotos...) a janela em primeiro plano pertence ao `ApplicationFrameHost.exe`,
//! e o app de verdade é uma janela FILHA, de classe `Windows.UI.Core.CoreWindow`, de outro processo. Quem olha só o dono da
//! janela em primeiro plano enxerga o hospedeiro e nunca o app; uma regra de "protegido" para um app UWP não casaria.
//!
//! Esta decisão é pura (sem Windows): recebe as janelas filhas e diz de qual processo é o app. Só entram classes de janela e
//! números de processo; nunca títulos. Na dúvida devolve `None` (o Guard trata como desconhecido e bloqueia: falha fechada).

/// Nome do executável do hospedeiro, em minúsculas.
pub const FRAME_HOST_EXE: &str = "applicationframehost.exe";

/// Classe da janela do app dentro da moldura do hospedeiro.
pub const CORE_WINDOW_CLASS: &str = "Windows.UI.Core.CoreWindow";

/// Uma janela filha da moldura do hospedeiro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostedChild {
    /// Processo dono da janela filha.
    pub pid: u32,
    /// A janela é do tipo `Windows.UI.Core.CoreWindow` (a janela do app)?
    pub is_core_window: bool,
}

/// De que processo é o app hospedado? Só conta uma janela `CoreWindow` que NÃO seja do próprio hospedeiro. Se não houver
/// nenhuma (app ainda abrindo, suspenso) ou se houver de processos diferentes (ambíguo), devolve `None`.
pub fn resolve_hosted(frame_pid: u32, children: &[HostedChild]) -> Option<u32> {
    let mut found: Option<u32> = None;
    for c in children.iter().filter(|c| c.is_core_window && c.pid != frame_pid && c.pid != 0) {
        match found {
            None => found = Some(c.pid),
            Some(p) if p == c.pid => {}
            Some(_) => return None, // dois apps diferentes: não dá para saber qual está na frente
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: u32 = 100;
    fn child(pid: u32, core: bool) -> HostedChild {
        HostedChild { pid, is_core_window: core }
    }

    #[test]
    fn the_app_is_the_core_window_owned_by_another_process() {
        // Como na Calculadora: duas janelas da barra de título e uma de entrada do hospedeiro, e a do app.
        let kids = [child(FRAME, false), child(FRAME, false), child(777, true), child(FRAME, false)];
        assert_eq!(resolve_hosted(FRAME, &kids), Some(777));
    }

    #[test]
    fn the_same_app_with_several_core_windows_is_still_one_app() {
        assert_eq!(resolve_hosted(FRAME, &[child(777, true), child(777, true)]), Some(777));
    }

    #[test]
    fn windows_of_the_host_itself_never_count_even_if_they_look_like_core_windows() {
        assert_eq!(resolve_hosted(FRAME, &[child(FRAME, true), child(FRAME, false)]), None);
    }

    #[test]
    fn no_core_window_yet_is_unknown_not_a_guess() {
        assert_eq!(resolve_hosted(FRAME, &[]), None);
        assert_eq!(resolve_hosted(FRAME, &[child(FRAME, false), child(900, false)]), None, "a child that is not a core window is not the app");
    }

    #[test]
    fn two_different_apps_are_ambiguous_and_fail_closed() {
        assert_eq!(resolve_hosted(FRAME, &[child(777, true), child(888, true)]), None);
    }

    #[test]
    fn a_zero_process_id_is_never_an_app() {
        assert_eq!(resolve_hosted(FRAME, &[child(0, true)]), None);
    }
}
