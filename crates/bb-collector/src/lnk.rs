//! Leitor MÍNIMO de atalhos do Windows (`.lnk`, formato MS-SHLLINK): extrai só o programa de destino.
//!
//! Puro e sem COM nem Shell: só lê bytes, então é determinístico, não resolve nada (não toca em rede, em
//! unidades externas nem no destino do atalho) e nunca entra em pânico com arquivo corrompido ou malicioso: toda
//! leitura é verificada e devolve `None` se algo estiver fora do lugar. Só o NOME do executável sai daqui.

use crate::apps::exe_from_icon_path;

/// Atalhos têm poucos KB; acima disto o arquivo é ignorado.
pub const MAX_LNK_BYTES: usize = 64 * 1024;

const HEADER_SIZE: usize = 0x4C;
// {00021401-0000-0000-C000-000000000046}
const LINK_CLSID: [u8; 16] = [0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46];

const HAS_LINK_TARGET_ID_LIST: u32 = 1 << 0;
const HAS_LINK_INFO: u32 = 1 << 1;
const HAS_NAME: u32 = 1 << 2;
const HAS_RELATIVE_PATH: u32 = 1 << 3;
const HAS_WORKING_DIR: u32 = 1 << 4;
const HAS_ARGUMENTS: u32 = 1 << 5;
const HAS_ICON_LOCATION: u32 = 1 << 6;
const IS_UNICODE: u32 = 1 << 7;

const ENV_BLOCK_SIGNATURE: u32 = 0xA000_0001;
const ENV_BLOCK_SIZE: usize = 0x314;

/// Leitura com limites: cada acesso fora do arquivo vira `None`.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn slice(&self, at: usize, len: usize) -> Option<&'a [u8]> {
        self.0.get(at..at.checked_add(len)?)
    }
    fn u16(&self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.slice(at, 2)?.try_into().ok()?))
    }
    fn u32(&self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.slice(at, 4)?.try_into().ok()?))
    }
    /// Texto ANSI terminado em NUL (cada byte vira um caractere: só o nome do arquivo interessa).
    fn ansi_z(&self, at: usize) -> Option<String> {
        let rest = self.0.get(at..)?;
        let end = rest.iter().position(|&b| b == 0)?;
        Some(rest[..end].iter().map(|&b| b as char).collect())
    }
    /// Texto UTF-16 terminado em NUL.
    fn utf16_z(&self, at: usize) -> Option<String> {
        let rest = self.0.get(at..)?;
        let mut units = Vec::new();
        for pair in rest.chunks_exact(2) {
            let unit = u16::from_le_bytes([pair[0], pair[1]]);
            if unit == 0 {
                return Some(String::from_utf16_lossy(&units));
            }
            units.push(unit);
        }
        None // sem terminador dentro do arquivo
    }
}

/// Caminho do destino do atalho, se o arquivo for um atalho válido que traz um. Prefere os campos Unicode.
pub fn target_of(data: &[u8]) -> Option<String> {
    if data.len() > MAX_LNK_BYTES {
        return None;
    }
    let b = Bytes(data);
    if b.u32(0)? as usize != HEADER_SIZE || b.slice(4, 16)? != LINK_CLSID {
        return None;
    }
    let flags = b.u32(0x14)?;
    let mut pos = HEADER_SIZE;

    let mut id_list: Option<&[u8]> = None;
    if flags & HAS_LINK_TARGET_ID_LIST != 0 {
        let len = b.u16(pos)? as usize;
        id_list = b.slice(pos.checked_add(2)?, len);
        pos = pos.checked_add(2)?.checked_add(len)?;
    }

    let mut from_link_info = None;
    if flags & HAS_LINK_INFO != 0 {
        let size = b.u32(pos)? as usize;
        if size < 0x1C {
            return None;
        }
        b.slice(pos, size)?; // o bloco inteiro precisa caber no arquivo
        from_link_info = link_info_target(&b, pos, size);
        pos = pos.checked_add(size)?;
    }

    // StringData: cada texto é "contador de caracteres + caracteres".
    let unicode = flags & IS_UNICODE != 0;
    for flag in [HAS_NAME, HAS_RELATIVE_PATH, HAS_WORKING_DIR, HAS_ARGUMENTS, HAS_ICON_LOCATION] {
        if flags & flag != 0 {
            let chars = b.u16(pos)? as usize;
            pos = pos.checked_add(2)?.checked_add(if unicode { chars.checked_mul(2)? } else { chars })?;
        }
    }

    // Blocos extras: o de variáveis de ambiente guarda o destino quando ele usa algo como %windir%.
    let mut from_env = None;
    while let Some(size) = b.u32(pos) {
        if size < 4 {
            break;
        }
        let size = size as usize;
        if b.u32(pos.checked_add(4)?) == Some(ENV_BLOCK_SIGNATURE) && size == ENV_BLOCK_SIZE {
            from_env = b.utf16_z(pos + 8 + 260).filter(|s| !s.is_empty()).or_else(|| b.ansi_z(pos + 8).filter(|s| !s.is_empty()));
        }
        pos = pos.checked_add(size)?;
    }

    // Sem caminho local nem variável de ambiente: o destino pode estar só na lista de identificadores do shell.
    from_link_info.or(from_env).or_else(|| id_list.and_then(exe_name_in_id_list))
}

/// Último item da lista de identificadores do shell: é o próprio arquivo do destino.
fn last_id_item(list: &[u8]) -> Option<&[u8]> {
    let (mut pos, mut last) = (0usize, None);
    loop {
        let size = u16::from_le_bytes(list.get(pos..pos + 2)?.try_into().ok()?) as usize;
        if size == 0 {
            return last;
        }
        let end = pos.checked_add(size)?;
        if size < 2 || end > list.len() {
            return last; // item truncado: usa o que foi lido antes
        }
        last = Some(&list[pos + 2..end]);
        pos = end;
    }
}

/// Nome do `.exe` guardado no último item da lista (texto UTF-16 ou ANSI terminado em NUL que acaba em `.exe`).
/// Não interpreta a estrutura do item (ela varia entre versões do Windows): procura o texto e devolve o MAIOR achado,
/// que é o nome longo do arquivo. Só devolve um nome de arquivo, sem pasta.
fn exe_name_in_id_list(list: &[u8]) -> Option<String> {
    let item = last_id_item(list)?;
    // Dois grupos: o texto UTF-16 (nome longo) e o ANSI (que pode ser o apelido 8.3, como SOFFIC~1.EXE). O nome
    // longo vence sempre; o ANSI só serve quando não há UTF-16.
    let (mut wide, mut narrow): (Option<String>, Option<String>) = (None, None);
    let valid = |name: &str| !name.is_empty() && !name.contains(['\\', '/', ':']) && name.to_lowercase().ends_with(".exe");
    let longer = |best: &Option<String>, name: &str| best.as_ref().is_none_or(|b| name.chars().count() > b.chars().count());
    // UTF-16: ".exe" seguido de NUL, andando para trás enquanto forem caracteres imprimíveis.
    for start in 0..item.len().saturating_sub(9) {
        let tail = &item[start..start + 10.min(item.len() - start)];
        let is_ext = tail.len() >= 10
            && tail[0] == b'.'
            && tail[1] == 0
            && tail[2].eq_ignore_ascii_case(&b'e')
            && tail[3] == 0
            && tail[4].eq_ignore_ascii_case(&b'x')
            && tail[5] == 0
            && tail[6].eq_ignore_ascii_case(&b'e')
            && tail[7] == 0
            && tail[8] == 0
            && tail[9] == 0;
        if !is_ext {
            continue;
        }
        let mut units = vec![0x2Eu16, 'e' as u16, 'x' as u16, 'e' as u16];
        let mut at = start;
        // O nome nunca começa antes do fim da assinatura do bloco de extensão (04 00 EF BE): os bytes dela e
        // dos campos fixos não fazem parte dele, mesmo que pareçam caracteres.
        let floor = item[..start].windows(4).rposition(|w| w == [0x04, 0x00, 0xEF, 0xBE]).map_or(0, |k| k + 4);
        while at >= 2 && at - 2 >= floor && units.len() < 260 {
            let u = u16::from_le_bytes([item[at - 2], item[at - 1]]);
            if u < 0x20 || u == 0xFFFF {
                break;
            }
            units.insert(0, u);
            at -= 2;
        }
        let name = String::from_utf16_lossy(&units);
        if valid(&name) && longer(&wide, &name) {
            wide = Some(name);
        }
    }
    // ANSI: ".exe" seguido de NUL.
    for start in 0..item.len().saturating_sub(4) {
        if item[start] == b'.' && item[start + 1..start + 4].eq_ignore_ascii_case(b"exe") && item.get(start + 4) == Some(&0) {
            let mut at = start;
            while at > 0 && start + 4 - at < 260 && item[at - 1] >= 0x20 && item[at - 1] < 0x7F {
                at -= 1;
            }
            let name: String = item[at..start + 4].iter().map(|&c| c as char).collect();
            if valid(&name) && longer(&narrow, &name) {
                narrow = Some(name);
            }
        }
    }
    wide.or(narrow)
}

/// Destino montado a partir do bloco LinkInfo (caminho local base + sufixo).
fn link_info_target(b: &Bytes, at: usize, size: usize) -> Option<String> {
    let header = b.u32(at + 4)? as usize;
    let flags = b.u32(at + 8)?;
    if flags & 1 == 0 {
        return None; // só destino em rede: sem caminho local
    }
    let within = |off: u32| -> Option<usize> {
        let off = off as usize;
        (off != 0 && off < size).then(|| at + off)
    };
    let ansi_base = within(b.u32(at + 16)?).and_then(|p| b.ansi_z(p));
    let ansi_suffix = within(b.u32(at + 24)?).and_then(|p| b.ansi_z(p)).unwrap_or_default();
    let (unicode_base, unicode_suffix) = if header >= 0x24 {
        (within(b.u32(at + 28)?).and_then(|p| b.utf16_z(p)), within(b.u32(at + 32)?).and_then(|p| b.utf16_z(p)).unwrap_or_default())
    } else {
        (None, String::new())
    };
    match (unicode_base, ansi_base) {
        (Some(base), _) => Some(base + &unicode_suffix),
        (None, Some(base)) => Some(base + &ansi_suffix),
        _ => None,
    }
}

/// Nome do executável (minúsculas, `.exe`) para onde o atalho aponta.
pub fn exe_of(data: &[u8]) -> Option<String> {
    exe_from_icon_path(&target_of(data)?)
}

#[cfg(test)]
pub(crate) mod fixtures {
    //! Montagem de atalhos sintéticos para os testes (nenhum arquivo real é usado).
    use super::*;

    fn put_u16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_le_bytes());
    }
    fn put_u32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_le_bytes());
    }

    #[derive(Default, Clone)]
    pub struct Lnk {
        pub id_list: Option<Vec<u8>>,
        pub base_ansi: Option<String>,
        pub suffix_ansi: String,
        pub base_unicode: Option<String>,
        pub suffix_unicode: String,
        pub name: Option<String>,
        pub arguments: Option<String>,
        pub env_target: Option<String>,
        pub unicode_strings: bool,
    }

    impl Lnk {
        pub fn local(base: &str) -> Lnk {
            Lnk { base_ansi: Some(base.to_owned()), ..Lnk::default() }
        }

        pub fn build(&self) -> Vec<u8> {
            let has_info = self.base_ansi.is_some() || self.base_unicode.is_some();
            let mut flags = 0u32;
            if self.id_list.is_some() {
                flags |= HAS_LINK_TARGET_ID_LIST;
            }
            if has_info {
                flags |= HAS_LINK_INFO;
            }
            if self.name.is_some() {
                flags |= HAS_NAME;
            }
            if self.arguments.is_some() {
                flags |= HAS_ARGUMENTS;
            }
            if self.unicode_strings {
                flags |= IS_UNICODE;
            }

            let mut v = Vec::new();
            put_u32(&mut v, HEADER_SIZE as u32);
            v.extend_from_slice(&LINK_CLSID);
            put_u32(&mut v, flags);
            v.resize(HEADER_SIZE, 0);

            if let Some(list) = &self.id_list {
                put_u16(&mut v, list.len() as u16);
                v.extend_from_slice(list);
            }

            if has_info {
                let unicode = self.base_unicode.is_some();
                let header = if unicode { 0x24 } else { 0x1C };
                let mut info = Vec::new();
                let mut body = Vec::new();
                let off = |body: &Vec<u8>| (header + body.len()) as u32;
                let base_off = off(&body);
                body.extend_from_slice(self.base_ansi.clone().unwrap_or_default().as_bytes());
                body.push(0);
                let suffix_off = off(&body);
                body.extend_from_slice(self.suffix_ansi.as_bytes());
                body.push(0);
                let (ub, us) = if unicode {
                    let b = off(&body);
                    for u in self.base_unicode.clone().unwrap_or_default().encode_utf16() {
                        put_u16(&mut body, u);
                    }
                    put_u16(&mut body, 0);
                    let s = off(&body);
                    for u in self.suffix_unicode.encode_utf16() {
                        put_u16(&mut body, u);
                    }
                    put_u16(&mut body, 0);
                    (b, s)
                } else {
                    (0, 0)
                };
                put_u32(&mut info, (header + body.len()) as u32);
                put_u32(&mut info, header as u32);
                put_u32(&mut info, 1); // VolumeIDAndLocalBasePath
                put_u32(&mut info, 0); // VolumeIDOffset (não usado aqui)
                put_u32(&mut info, base_off);
                put_u32(&mut info, 0); // CommonNetworkRelativeLinkOffset
                put_u32(&mut info, suffix_off);
                if unicode {
                    put_u32(&mut info, ub);
                    put_u32(&mut info, us);
                }
                info.extend_from_slice(&body);
                v.extend_from_slice(&info);
            }

            let text = |v: &mut Vec<u8>, s: &str| {
                if self.unicode_strings {
                    put_u16(v, s.encode_utf16().count() as u16);
                    for u in s.encode_utf16() {
                        put_u16(v, u);
                    }
                } else {
                    put_u16(v, s.len() as u16);
                    v.extend_from_slice(s.as_bytes());
                }
            };
            if let Some(n) = &self.name {
                text(&mut v, n);
            }
            if let Some(a) = &self.arguments {
                text(&mut v, a);
            }

            if let Some(target) = &self.env_target {
                put_u32(&mut v, ENV_BLOCK_SIZE as u32);
                put_u32(&mut v, ENV_BLOCK_SIGNATURE);
                let mut ansi = target.as_bytes().to_vec();
                ansi.resize(260, 0);
                v.extend_from_slice(&ansi);
                let mut wide: Vec<u8> = target.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                wide.resize(520, 0);
                v.extend_from_slice(&wide);
            }
            put_u32(&mut v, 0); // fim dos blocos extras
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::Lnk;
    use super::*;

    #[test]
    fn reads_a_local_target_with_and_without_a_suffix() {
        let l = Lnk::local("C:\\Program Files\\App\\app.exe");
        assert_eq!(target_of(&l.build()).as_deref(), Some("C:\\Program Files\\App\\app.exe"));
        let split = Lnk { base_ansi: Some("C:\\Program Files\\App".into()), suffix_ansi: "tool.exe".into(), ..Lnk::default() };
        // O formato real separa base e sufixo; sem barra entre eles a leitura apenas concatena.
        assert_eq!(exe_of(&Lnk { suffix_ansi: "\\tool.exe".into(), ..split.clone() }.build()).as_deref(), Some("tool.exe"));
    }

    #[test]
    fn prefers_the_unicode_fields_when_present() {
        let l = Lnk {
            base_ansi: Some("C:\\Program Files\\Caf?\\cafe.exe".into()),
            base_unicode: Some("C:\\Program Files\\Café\\café.exe".into()),
            ..Lnk::default()
        };
        assert_eq!(target_of(&l.build()).as_deref(), Some("C:\\Program Files\\Café\\café.exe"));
        assert_eq!(exe_of(&l.build()).as_deref(), Some("café.exe"));
    }

    #[test]
    fn reads_the_target_from_the_environment_block_when_there_is_no_link_info() {
        let l = Lnk { env_target: Some("%windir%\\system32\\notepad.exe".into()), id_list: Some(vec![1, 2, 3, 4]), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()).as_deref(), Some("notepad.exe"));
    }

    #[test]
    fn skips_the_id_list_and_the_string_data_in_either_encoding() {
        for unicode in [false, true] {
            let l = Lnk {
                id_list: Some(vec![0xAB; 37]),
                base_ansi: Some("C:\\x\\game.exe".into()),
                name: Some("Description of the shortcut".into()),
                arguments: Some("--launch --fullscreen".into()),
                env_target: Some("%x%\\other.exe".into()),
                unicode_strings: unicode,
                ..Lnk::default()
            };
            assert_eq!(exe_of(&l.build()).as_deref(), Some("game.exe"), "unicode strings: {unicode}");
        }
    }

    #[test]
    fn shortcuts_to_something_else_have_no_program() {
        assert_eq!(exe_of(&Lnk::local("C:\\x\\readme.txt").build()), None);
        assert_eq!(exe_of(&Lnk::local("C:\\x\\folder\\").build()), None);
        assert_eq!(exe_of(&Lnk::default().build()), None, "no target information at all");
    }

    #[test]
    fn rejects_files_that_are_not_shortcuts() {
        let good = Lnk::local("C:\\x\\a.exe").build();
        assert!(exe_of(&good).is_some());
        let mut wrong_header = good.clone();
        wrong_header[0] = 0x4D;
        let mut wrong_clsid = good.clone();
        wrong_clsid[4] ^= 0xFF;
        for bad in [Vec::new(), vec![0u8; 3], b"MZ\x90\x00 this is an executable, not a shortcut".to_vec(), wrong_header, wrong_clsid] {
            assert_eq!(exe_of(&bad), None);
        }
        assert_eq!(exe_of(&vec![0x41u8; MAX_LNK_BYTES + 1]), None, "oversized files are ignored");
    }

    // Nenhum arquivo cortado ou adulterado pode causar pânico; no pior caso devolve `None`.
    #[test]
    fn truncated_and_corrupted_files_never_panic() {
        let samples = [
            Lnk::local("C:\\Program Files\\App\\app.exe").build(),
            Lnk {
                base_unicode: Some("C:\\Café\\café.exe".into()),
                base_ansi: Some("C:\\Caf?\\caf?.exe".into()),
                name: Some("n".into()),
                unicode_strings: true,
                ..Lnk::default()
            }
            .build(),
            Lnk { env_target: Some("%windir%\\notepad.exe".into()), id_list: Some(vec![9; 20]), ..Lnk::default() }.build(),
        ];
        for sample in samples {
            for cut in 0..=sample.len() {
                let _ = exe_of(&sample[..cut]);
            }
            // Embaralhamento determinístico de bytes (gerador congruencial simples).
            let mut seed = 0x1234_5678u32;
            for _ in 0..4000 {
                let mut m = sample.clone();
                for _ in 0..3 {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let i = (seed >> 8) as usize % m.len();
                    m[i] = (seed >> 24) as u8;
                }
                let _ = exe_of(&m);
            }
        }
    }

    /// Lista de identificadores com os itens dados (cada item: bytes de dados; o tamanho e o terminador são postos aqui).
    fn id_list(items: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        for it in items {
            out.extend_from_slice(&((it.len() + 2) as u16).to_le_bytes());
            out.extend_from_slice(it);
        }
        out.extend_from_slice(&[0, 0]);
        out
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect()
    }

    /// Item de arquivo sintético: tipo, campos fixos e o nome longo em UTF-16 mais um nome curto ANSI.
    fn file_item(short_ansi: &str, long_unicode: &str) -> Vec<u8> {
        let mut v = vec![0x32, 0x00];
        v.extend_from_slice(&[0u8; 10]); // tamanho, data e atributos
        v.extend_from_slice(short_ansi.as_bytes());
        v.push(0);
        v.extend_from_slice(&[0x1E, 0x00, 0x04, 0x00, 0xEF, 0xBE]); // início de um bloco de extensão
        v.extend_from_slice(&utf16z(long_unicode));
        v
    }

    #[test]
    fn reads_the_program_from_the_shell_id_list_when_there_is_no_path() {
        let folder = file_item("PROGRA~1", "Program Files");
        let l = Lnk { id_list: Some(id_list(&[folder, file_item("SOFFIC~1.EXE", "soffice.exe")])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()).as_deref(), Some("soffice.exe"));
        // O nome longo vence o curto.
        let l = Lnk { id_list: Some(id_list(&[file_item("MYPROG~1.EXE", "My Long Program Name.exe")])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()).as_deref(), Some("my long program name.exe"));
        // Só o nome curto ANSI.
        let mut ansi_only = vec![0x32, 0x00];
        ansi_only.extend_from_slice(&[0u8; 10]);
        ansi_only.extend_from_slice(b"tool.exe\0");
        let l = Lnk { id_list: Some(id_list(&[ansi_only])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()).as_deref(), Some("tool.exe"));
    }

    #[test]
    fn the_id_list_is_only_used_for_exe_files_and_only_the_last_item_counts() {
        // O último item é uma pasta ou um documento: sem programa, mesmo que um item ANTERIOR seja um .exe.
        let l = Lnk { id_list: Some(id_list(&[file_item("A~1.EXE", "decoy.exe"), file_item("DOC~1.TXT", "notes.txt")])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()), None);
        let l = Lnk { id_list: Some(id_list(&[file_item("FOLDER~1", "Some Folder")])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()), None);
        // Um caminho completo dentro do texto não vira nome de arquivo.
        let l = Lnk { id_list: Some(id_list(&[file_item("X~1", "C:\\evil\\path.exe")])), ..Lnk::default() };
        assert_eq!(exe_of(&l.build()), None);
    }

    #[test]
    fn a_path_in_the_link_info_wins_over_the_id_list() {
        let l = Lnk {
            id_list: Some(id_list(&[file_item("X~1.EXE", "from-id-list.exe")])),
            base_ansi: Some("C:\\x\\from-link-info.exe".into()),
            ..Lnk::default()
        };
        assert_eq!(exe_of(&l.build()).as_deref(), Some("from-link-info.exe"));
    }

    #[test]
    fn malformed_id_lists_never_panic() {
        let good =
            Lnk { id_list: Some(id_list(&[file_item("A~1", "folder"), file_item("B~1.EXE", "real.exe")])), ..Lnk::default() }.build();
        for cut in 0..=good.len() {
            let _ = exe_of(&good[..cut]);
        }
        let mut seed = 99u32;
        for _ in 0..4000 {
            let mut m = good.clone();
            for _ in 0..4 {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let i = (seed >> 8) as usize % m.len();
                m[i] = (seed >> 24) as u8;
            }
            let _ = exe_of(&m);
        }
        // Tamanhos de item absurdos ou zero.
        for first in [[0xFFu8, 0xFF], [1, 0], [2, 0], [3, 0]] {
            let l = Lnk { id_list: Some([&first[..], &[0u8; 30][..]].concat()), ..Lnk::default() };
            let _ = exe_of(&l.build());
        }
    }

    #[test]
    fn absurd_sizes_inside_the_file_do_not_overflow_or_loop() {
        let mut l = Lnk::local("C:\\x\\a.exe").build();
        // LinkInfoSize enorme logo depois do cabeçalho (sem ID list): 0xFFFFFFFF.
        l[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(exe_of(&l), None);
        // Bloco extra com tamanho gigante.
        let mut e = Lnk { env_target: Some("%a%\\b.exe".into()), ..Lnk::default() }.build();
        let n = e.len();
        e[n - 4..].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
        let _ = exe_of(&e);
    }
}
