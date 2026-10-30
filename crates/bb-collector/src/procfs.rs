//! Fonte de processos para Linux, a partir do `/proc`, sem privilégios (issue #126).
//!
//! Entrega os MESMOS campos do coletor do Windows (`ProcessSample`): chave `(pid, início)`, nome do executável, pai, tempo de CPU e
//! memória. Só lê `stat` e `statm` de cada processo e o link `exe`; do sistema, `stat` e `meminfo`. NUNCA lê `cmdline`, `environ`,
//! `cwd`, `fd`, `maps` nem qualquer outro arquivo: um teste falha se a fonte pedir qualquer coisa fora dessa lista.
//!
//! Todo o texto do `/proc` é interpretado por funções puras (testáveis em qualquer sistema, com fixtures sintéticas). A leitura do
//! disco fica atrás de [`ProcFs`], então os testes usam um `/proc` falso e o Linux usa [`RealProc`].
//!
//! Decisões, medidas numa sessão Linux real (ver a pesquisa da #112):
//! - O nome vem do link `exe` (o arquivo apontado), NUNCA do `comm`: o `comm` trunca em 15 caracteres e o próprio processo pode mudá-lo.
//! - Sem permissão para ler `exe` (processos de outros usuários, threads de kernel), o processo é DESCARTADO e só contado: o esquema
//!   fechado exige nome, e adivinhar um nome seria pior do que não gravar (mesma conduta do Windows com processos protegidos).
//! - Um script aparece pelo interpretador (`dash`, `python3`, `node`), não pelo nome do script: é uma limitação documentada.
//! - O horário de início é relativo ao boot do sistema + o `btime` lido UMA vez por execução do app. Reler o `btime` a cada ciclo faria
//!   a chave `(pid, início)` mudar quando o relógio é ajustado, e cada ajuste pareceria "todos os processos recomeçaram".

use std::path::PathBuf;

use bb_core::{ExeName, ProcessKey};

use crate::sample::{CollectError, ProcessSample, ProcessSource, SystemSample};

/// Tiques por segundo dos tempos que o kernel exporta em `/proc` (`USER_HZ`): é fixo em 100 nas arquiteturas comuns.
pub const USER_HZ: u64 = 100;

/// Teto de processos examinados por ciclo (um `/proc` com milhões de entradas não trava o ciclo).
pub const MAX_PROCESSES: usize = 100_000;

/// Campos que interessam de `/proc/<pid>/stat`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatFields {
    pub ppid: u32,
    /// Tempo em modo usuário, em tiques.
    pub utime: u64,
    /// Tempo em modo kernel, em tiques.
    pub stime: u64,
    /// Início do processo, em tiques desde o boot.
    pub starttime: u64,
}

/// Interpreta `/proc/<pid>/stat`. O nome (`comm`) fica entre parênteses e pode ter espaços e até parênteses: o corte é do primeiro `(`
/// ao ÚLTIMO `)`. Qualquer defeito devolve `None` (o processo é descartado), nunca um número inventado.
pub fn parse_stat(raw: &str) -> Option<StatFields> {
    let close = raw.rfind(')')?;
    raw.find('(').filter(|open| *open < close)?;
    let mut f = raw[close + 1..].split_whitespace();
    // Depois do ")": state, ppid, pgrp, session, tty_nr, tpgid, flags, minflt, cminflt, majflt, cmajflt, utime, stime, ..., starttime.
    let _state = f.next()?;
    let ppid = f.next()?.parse().ok()?;
    let mut rest = f.skip(9); // pgrp .. cmajflt
    let utime = rest.next()?.parse().ok()?;
    let stime = rest.next()?.parse().ok()?;
    let starttime = rest.nth(6)?.parse().ok()?; // cutime, cstime, priority, nice, num_threads, itrealvalue
    Some(StatFields { ppid, utime, stime, starttime })
}

/// Páginas residentes (2º campo de `/proc/<pid>/statm`).
pub fn parse_statm_rss_pages(raw: &str) -> Option<u64> {
    raw.split_whitespace().nth(1)?.parse().ok()
}

/// Horário de boot (segundos desde 1970) da linha `btime` de `/proc/stat`.
pub fn parse_btime(proc_stat: &str) -> Option<u64> {
    proc_stat.lines().find_map(|l| l.strip_prefix("btime ")).and_then(|v| v.trim().parse().ok())
}

/// Tempo de CPU acumulado (usuário + kernel) em unidades de 100 ns, como o Windows.
pub fn cpu_time_100ns(utime: u64, stime: u64) -> u64 {
    utime.saturating_add(stime).saturating_mul(10_000_000 / USER_HZ)
}

/// Início do processo em milissegundos desde 1970: `btime` + `starttime` (tiques desde o boot). `None` se a conta estoura.
pub fn start_time_ms(btime_s: u64, starttime_ticks: u64) -> Option<i64> {
    let ms = btime_s.checked_mul(1000)?.checked_add(starttime_ticks.checked_mul(1000 / USER_HZ)?)?;
    i64::try_from(ms).ok()
}

/// Memória residente em KB.
pub fn rss_kb(pages: u64, page_size: u64) -> u64 {
    pages.saturating_mul(page_size) / 1024
}

/// Nome do executável a partir do alvo do link `/proc/<pid>/exe`: só o NOME do arquivo (nunca a pasta), em minúsculas, validado como o
/// Guard validaria. O kernel acrescenta ` (deleted)` quando o binário foi substituído com o processo rodando: o sufixo sai.
pub fn exe_name_from_link(target: &str) -> Option<ExeName> {
    let target = target.strip_suffix(" (deleted)").unwrap_or(target);
    let name = target.rsplit('/').next()?;
    ExeName::new(name).ok()
}

/// Tempos de CPU do sistema (`/proc/stat`, linha `cpu`): `(ocupado, total)` em tiques.
/// Ocioso = `idle` + `iowait`; total = soma dos oito primeiros campos (user, nice, system, idle, iowait, irq, softirq, steal).
pub fn parse_cpu_times(proc_stat: &str) -> Option<(u64, u64)> {
    let line = proc_stat.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).take(8).map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().sum();
    let idle = v[3].saturating_add(v.get(4).copied().unwrap_or(0));
    Some((total.saturating_sub(idle), total))
}

/// Uso da CPU em permilagem entre duas leituras (`(ocupado, total)`); sem diferença de tempo, 0.
pub fn cpu_permille(prev: (u64, u64), now: (u64, u64)) -> u16 {
    let total = now.1.saturating_sub(prev.1);
    if total == 0 {
        return 0;
    }
    (now.0.saturating_sub(prev.0).saturating_mul(1000) / total).min(1000) as u16
}

/// Memória total e usada (em KB) de `/proc/meminfo`. Usada = total - disponível (`MemAvailable`); em kernels sem essa linha, livre +
/// buffers + cache.
pub fn parse_meminfo(raw: &str) -> Option<(u64, u64)> {
    let get = |key: &str| -> Option<u64> {
        raw.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix(':')?.split_whitespace().next()?.parse().ok())
    };
    let total = get("MemTotal")?;
    let available = match get("MemAvailable") {
        Some(a) => a,
        None => get("MemFree")?.saturating_add(get("Buffers").unwrap_or(0)).saturating_add(get("Cached").unwrap_or(0)),
    };
    Some((total, total.saturating_sub(available.min(total))))
}

/// O que a fonte precisa do `/proc`. Só estes quatro acessos existem: nada mais pode ser lido.
pub trait ProcFs {
    /// Números de processo existentes agora. Erro = `/proc` indisponível.
    fn pids(&self) -> Result<Vec<u32>, CollectError>;
    /// `stat` ou `statm` de um processo (o único texto por processo que a fonte lê).
    fn pid_file(&self, pid: u32, name: &str) -> Option<String>;
    /// Alvo do link `exe` de um processo (`None` sem permissão, thread de kernel ou processo que sumiu).
    fn exe_link(&self, pid: u32) -> Option<String>;
    /// `stat` ou `meminfo` do sistema.
    fn system_file(&self, name: &str) -> Option<String>;
}

/// O `/proc` de verdade (ou outra pasta com o mesmo formato).
pub struct RealProc {
    root: PathBuf,
}

impl RealProc {
    pub fn new() -> Self {
        Self { root: PathBuf::from("/proc") }
    }
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Default for RealProc {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcFs for RealProc {
    fn pids(&self) -> Result<Vec<u32>, CollectError> {
        let rd = std::fs::read_dir(&self.root).map_err(|e| CollectError(format!("/proc: {e}")))?;
        Ok(rd.flatten().filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok()).take(MAX_PROCESSES).collect())
    }
    fn pid_file(&self, pid: u32, name: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(pid.to_string()).join(name)).ok()
    }
    fn exe_link(&self, pid: u32) -> Option<String> {
        std::fs::read_link(self.root.join(pid.to_string()).join("exe")).ok()?.to_str().map(str::to_owned)
    }
    fn system_file(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(name)).ok()
    }
}

/// Fonte de processos sobre um [`ProcFs`].
pub struct ProcfsProcessSource<F: ProcFs> {
    fs: F,
    page_size: u64,
    /// Lido UMA vez: ver a nota do módulo.
    btime_s: Option<u64>,
    prev_cpu: Option<(u64, u64)>,
    dropped: usize,
}

impl<F: ProcFs> ProcfsProcessSource<F> {
    pub fn new(fs: F, page_size: u64) -> Self {
        Self { fs, page_size: page_size.max(1), btime_s: None, prev_cpu: None, dropped: 0 }
    }

    /// Quantos processos o último ciclo descartou por não terem nome legível (só um número, nunca quais).
    pub fn dropped_last_cycle(&self) -> usize {
        self.dropped
    }

    fn btime(&mut self) -> Option<u64> {
        if self.btime_s.is_none() {
            self.btime_s = parse_btime(&self.fs.system_file("stat")?);
        }
        self.btime_s
    }
}

impl<F: ProcFs> ProcessSource for ProcfsProcessSource<F> {
    fn processes(&mut self) -> Result<Vec<ProcessSample>, CollectError> {
        let pids = self.fs.pids()?;
        let btime = self.btime().ok_or_else(|| CollectError("/proc/stat sem btime".into()))?;
        let (mut out, mut dropped) = (Vec::new(), 0usize);
        for pid in pids {
            let Some(exe_name) = self.fs.exe_link(pid).and_then(|l| exe_name_from_link(&l)) else {
                dropped += 1;
                continue;
            };
            let sample = (|| {
                let stat = parse_stat(&self.fs.pid_file(pid, "stat")?)?;
                let pages = parse_statm_rss_pages(&self.fs.pid_file(pid, "statm")?)?;
                Some(ProcessSample {
                    key: ProcessKey { pid, start_time_ms: start_time_ms(btime, stat.starttime)? },
                    exe_name,
                    parent_pid: stat.ppid,
                    cpu_time_100ns: cpu_time_100ns(stat.utime, stat.stime),
                    working_set_kb: rss_kb(pages, self.page_size),
                })
            })();
            match sample {
                Some(s) => out.push(s),
                None => dropped += 1,
            }
        }
        self.dropped = dropped;
        Ok(out)
    }

    fn system(&mut self) -> Result<SystemSample, CollectError> {
        let stat = self.fs.system_file("stat").ok_or_else(|| CollectError("/proc/stat".into()))?;
        let now = parse_cpu_times(&stat).ok_or_else(|| CollectError("/proc/stat sem linha cpu".into()))?;
        let cpu_permille = self.prev_cpu.map_or(0, |prev| cpu_permille(prev, now));
        self.prev_cpu = Some(now);
        let mem = self.fs.system_file("meminfo").ok_or_else(|| CollectError("/proc/meminfo".into()))?;
        let (total, used) = parse_meminfo(&mem).ok_or_else(|| CollectError("/proc/meminfo sem MemTotal".into()))?;
        Ok(SystemSample { cpu_permille, mem_used_kb: used, mem_total_kb: total })
    }
}

/// A fonte do Linux: o `/proc` real e o tamanho de página do sistema.
#[cfg(target_os = "linux")]
pub type LinuxProcessSource = ProcfsProcessSource<RealProc>;

#[cfg(target_os = "linux")]
impl LinuxProcessSource {
    pub fn linux() -> Self {
        // SAFETY: `sysconf` só lê uma constante do sistema.
        let page = unsafe { libc::sysconf(libc::_SC_PAGE_SIZE) };
        Self::new(RealProc::new(), if page > 0 { page as u64 } else { 4096 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    // ---- fixtures sintéticas (nada de dados reais) ----
    const STAT_SIMPLE: &str =
        "4242 (synth-app) S 100 4242 4242 0 -1 4194560 120 0 3 0 250 50 0 0 20 0 7 0 8000 123456789 2048 18446744073709551615";
    const STAT_WEIRD_COMM: &str = "77 (a (b) c) R 1 77 77 0 -1 0 0 0 0 0 10 20 0 0 20 0 1 0 500 1 1 1";
    const PROC_STAT: &str = "cpu  1000 200 300 5000 100 10 20 5 0 0\ncpu0 500 100 150 2500 50 5 10 2 0 0\nbtime 1790000000\nprocesses 99\n";
    const MEMINFO: &str = "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    6000000 kB\nBuffers:          500000 kB\nCached:          2000000 kB\n";

    #[test]
    fn stat_is_parsed_even_when_the_name_has_spaces_and_parentheses() {
        assert_eq!(parse_stat(STAT_SIMPLE), Some(StatFields { ppid: 100, utime: 250, stime: 50, starttime: 8000 }));
        assert_eq!(parse_stat(STAT_WEIRD_COMM), Some(StatFields { ppid: 1, utime: 10, stime: 20, starttime: 500 }));
    }

    #[test]
    fn a_malformed_stat_is_dropped_never_guessed() {
        for bad in [
            "",
            "garbage",
            "1 (x",
            "1 x) S 1",
            "1 (x) S",
            "1 (x) S abc 1 1 0 -1 0 0 0 0 0 1 2 0 0 20 0 1 0 5",
            "1 (x) S 1 1 1 0 -1 0 0 0 0 0 1 2 0 0 20 0 1 0",
        ] {
            assert_eq!(parse_stat(bad), None, "{bad:?}");
        }
        assert_eq!(parse_stat(")( weird"), None, "a ')' before the '(' is not a name");
    }

    #[test]
    fn time_and_memory_conversions_match_the_windows_units() {
        assert_eq!(cpu_time_100ns(250, 50), 300 * 100_000, "300 ticks of 10 ms = 3 s = 30,000,000 units of 100 ns");
        assert_eq!(start_time_ms(1_790_000_000, 8000), Some(1_790_000_000_000 + 80_000));
        assert_eq!(start_time_ms(u64::MAX, 1), None, "overflow is None");
        assert_eq!(rss_kb(2048, 4096), 8192);
        assert_eq!(rss_kb(u64::MAX, u64::MAX), u64::MAX / 1024, "never panics");
        assert_eq!(parse_statm_rss_pages("5000 2048 100 10 0 400 0"), Some(2048));
        assert_eq!(parse_statm_rss_pages("5000"), None);
        assert_eq!(parse_btime(PROC_STAT), Some(1_790_000_000));
        assert_eq!(parse_btime("cpu 1 2 3 4\n"), None);
    }

    #[test]
    fn the_executable_name_is_the_file_name_of_the_link_target_only() {
        for (link, want) in [
            ("/usr/bin/firefox", Some("firefox")),
            ("/opt/Google/Chrome/chrome", Some("chrome")),
            ("/usr/bin/KeePassXC", Some("keepassxc")),
            ("/home/someone/app/bin/my tool", Some("my tool")),
            ("/usr/lib/systemd/systemd-journald (deleted)", Some("systemd-journald")),
            ("/usr/bin/dash", Some("dash")),
        ] {
            assert_eq!(exe_name_from_link(link).as_ref().map(|n| n.as_str()), want, "{link}");
        }
        for bad in ["", "/", "/usr/bin/", "/x/\u{1}evil", " (deleted)"] {
            assert_eq!(exe_name_from_link(bad), None, "{bad:?}");
        }
        let long = format!("/bin/{}", "a".repeat(65));
        assert_eq!(exe_name_from_link(&long), None, "longer than the Guard accepts");
    }

    #[test]
    fn system_cpu_and_memory_are_parsed() {
        // user+nice+system+idle+iowait+irq+softirq+steal = 1000+200+300+5000+100+10+20+5 = 6635; ocioso = 5100.
        assert_eq!(parse_cpu_times(PROC_STAT), Some((1535, 6635)));
        assert_eq!(parse_cpu_times("cpuX 1 2 3 4"), None);
        assert_eq!(parse_cpu_times("cpu  1 2 3"), None);
        assert_eq!(cpu_permille((0, 0), (500, 1000)), 500);
        assert_eq!(cpu_permille((500, 1000), (500, 1000)), 0, "no time elapsed");
        assert_eq!(cpu_permille((10, 10), (5000, 100)), 1000, "capped");
        assert_eq!(parse_meminfo(MEMINFO), Some((16_000_000, 10_000_000)));
        let old_kernel = "MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 50 kB\nCached: 150 kB\n";
        assert_eq!(parse_meminfo(old_kernel), Some((1000, 700)));
        assert_eq!(parse_meminfo("MemFree: 1 kB"), None);
        assert_eq!(parse_meminfo("MemTotal: 10 kB\nMemAvailable: 99 kB"), Some((10, 0)), "available above total never underflows");
    }

    // ---- /proc falso, que registra tudo o que a fonte pede ----
    #[derive(Default)]
    struct FakeProc {
        pids: Vec<u32>,
        pid_files: HashMap<(u32, String), String>,
        links: HashMap<u32, String>,
        system: HashMap<String, String>,
        asked: RefCell<Vec<String>>,
        fail_pids: bool,
    }
    impl ProcFs for FakeProc {
        fn pids(&self) -> Result<Vec<u32>, CollectError> {
            if self.fail_pids {
                return Err(CollectError("/proc: unavailable".into()));
            }
            Ok(self.pids.clone())
        }
        fn pid_file(&self, pid: u32, name: &str) -> Option<String> {
            self.asked.borrow_mut().push(format!("pid:{name}"));
            self.pid_files.get(&(pid, name.to_owned())).cloned()
        }
        fn exe_link(&self, pid: u32) -> Option<String> {
            self.asked.borrow_mut().push("pid:exe".into());
            self.links.get(&pid).cloned()
        }
        fn system_file(&self, name: &str) -> Option<String> {
            self.asked.borrow_mut().push(format!("sys:{name}"));
            self.system.get(name).cloned()
        }
    }

    fn world() -> FakeProc {
        let mut w = FakeProc::default();
        w.system.insert("stat".into(), PROC_STAT.into());
        w.system.insert("meminfo".into(), MEMINFO.into());
        // 4242: processo do usuario; 1: de outro usuario (sem permissao de ler exe); 2: thread de kernel (sem link).
        for (pid, stat) in [(4242u32, STAT_SIMPLE), (77, STAT_WEIRD_COMM), (1, STAT_SIMPLE), (2, STAT_SIMPLE)] {
            w.pids.push(pid);
            w.pid_files.insert((pid, "stat".into()), stat.into());
            w.pid_files.insert((pid, "statm".into()), "5000 2048 100 10 0 400 0".into());
        }
        w.links.insert(4242, "/usr/bin/Synth-App".into());
        w.links.insert(77, "/usr/bin/a (b) c".into());
        w
    }

    fn source(w: FakeProc) -> ProcfsProcessSource<FakeProc> {
        ProcfsProcessSource::new(w, 4096)
    }

    #[test]
    fn processes_have_the_same_fields_as_the_windows_collector() {
        let mut s = source(world());
        let mut v = s.processes().unwrap();
        v.sort_by_key(|p| p.key.pid);
        assert_eq!(v.len(), 2, "only the two with a readable name");
        let p = &v[1];
        assert_eq!(p.key, ProcessKey { pid: 4242, start_time_ms: 1_790_000_080_000 });
        assert_eq!(p.exe_name.as_str(), "synth-app");
        assert_eq!((p.parent_pid, p.cpu_time_100ns, p.working_set_kb), (100, 30_000_000, 8192));
    }

    #[test]
    fn processes_without_a_readable_name_are_dropped_and_only_counted() {
        let mut s = source(world());
        assert_eq!(s.dropped_last_cycle(), 0);
        let v = s.processes().unwrap();
        assert!(v.iter().all(|p| p.key.pid == 4242 || p.key.pid == 77));
        assert_eq!(s.dropped_last_cycle(), 2, "other user's process and the kernel thread");
    }

    #[test]
    fn a_process_that_vanished_between_reads_is_dropped_without_error() {
        let mut w = world();
        w.pid_files.remove(&(4242, "statm".into()));
        w.pid_files.insert((77, "stat".into()), "77 (broken".into());
        let mut s = source(w);
        assert!(s.processes().unwrap().is_empty());
        assert_eq!(s.dropped_last_cycle(), 4);
    }

    #[test]
    fn only_the_allowed_files_are_ever_read() {
        let mut s = source(world());
        s.processes().unwrap();
        s.system().unwrap();
        let mut asked: Vec<String> = s.fs.asked.borrow().clone();
        asked.sort();
        asked.dedup();
        assert_eq!(asked, ["pid:exe", "pid:stat", "pid:statm", "sys:meminfo", "sys:stat"], "no cmdline, environ, cwd, fd or maps");
    }

    #[test]
    fn the_boot_time_is_read_once_so_a_clock_step_never_changes_the_keys() {
        let mut s = source(world());
        let first = s.processes().unwrap().into_iter().map(|p| p.key).collect::<Vec<_>>();
        s.fs.system.insert("stat".into(), PROC_STAT.replace("btime 1790000000", "btime 1790000999"));
        let second = s.processes().unwrap().into_iter().map(|p| p.key).collect::<Vec<_>>();
        assert_eq!(first, second, "the same instance keeps the same key even if /proc/stat now says another boot time");
    }

    #[test]
    fn an_unavailable_proc_is_an_error_not_an_empty_list() {
        let mut w = world();
        w.fail_pids = true;
        assert!(source(w).processes().is_err());
        let mut w = world();
        w.system.remove("stat");
        assert!(source(w).processes().is_err(), "no boot time, no reliable identity");
    }

    #[test]
    fn system_samples_use_the_difference_between_two_readings() {
        let mut s = source(world());
        let a = s.system().unwrap();
        assert_eq!((a.cpu_permille, a.mem_total_kb, a.mem_used_kb), (0, 16_000_000, 10_000_000), "the first reading has no reference");
        s.fs.system.insert("stat".into(), "cpu  2000 200 300 5600 100 10 20 5 0 0\nbtime 1790000000\n".into());
        let b = s.system().unwrap();
        // (ocupado, total): antes (1535, 6635); agora (2535, 8235): 1000 de 1600 tiques = 625 por mil... ocupado subiu 1000, total 1600.
        assert_eq!(b.cpu_permille, 625);
        s.fs.system.remove("meminfo");
        assert!(s.system().is_err());
    }
}
