//! Baixar e preparar uma atualização. Nada aqui executa o instalador.
//!
//! Regras (cada uma tem teste):
//!   * só se baixa a versão já validada, de endereços montados a partir dela (nunca de texto da rede);
//!   * a assinatura é buscada ANTES do instalador: sem `.sig` não se baixa nada (a Release ainda não foi assinada);
//!   * o instalador é gravado como `.partial` e só ganha o nome final DEPOIS de passar pelo SHA-256 + Ed25519;
//!   * qualquer falha apaga o que foi baixado: nunca sobra um instalador não verificado com nome de instalador.

use std::fs;
use std::path::{Path, PathBuf};

use crate::verify::{sha256_hex, trusted_keys, verify_release};
use crate::{Downloader, UpdateError, Version, OWNER, REPO};

pub const DOWNLOAD_HOST: &str = "github.com";
/// O instalador atual tem uns 3 MB; acima disto é recusado.
pub const MAX_INSTALLER: u64 = 64 * 1024 * 1024;
/// Uma assinatura Ed25519 em hexadecimal tem 128 caracteres.
pub const MAX_SIGNATURE: u64 = 1024;

/// Nome do instalador de uma versão (o mesmo que o workflow de release publica).
pub fn installer_name(version: &Version) -> String {
    format!("Developer-Black-Box_{version}_x64-setup.exe")
}

fn asset_path(version: &Version, name: &str) -> String {
    format!("/{OWNER}/{REPO}/releases/download/v{version}/{name}")
}

fn remove_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

/// A pasta de atualizações é só nossa: apaga restos de tentativas anteriores.
fn clean(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_file() {
                remove_quietly(&e.path());
            }
        }
    }
}

fn status_error(status: u16, not_found: UpdateError) -> UpdateError {
    if status == 404 {
        not_found
    } else {
        UpdateError::Status(status)
    }
}

/// Baixa `version` para `dir`, verifica e devolve o caminho do instalador já verificado.
pub fn prepare(version: &Version, dir: &Path, dl: &dyn Downloader) -> Result<PathBuf, UpdateError> {
    let keys = trusted_keys();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    prepare_with(&keys, version, dir, dl)
}

pub(crate) fn prepare_with(keys: &[&str], version: &Version, dir: &Path, dl: &dyn Downloader) -> Result<PathBuf, UpdateError> {
    fs::create_dir_all(dir).map_err(|_| UpdateError::Disk)?;
    clean(dir);

    let name = installer_name(version);
    let sig_name = format!("{name}.sig");
    let sig_part = dir.join(format!("{sig_name}.partial"));
    let exe_part = dir.join(format!("{name}.partial"));
    let cleanup = || {
        remove_quietly(&sig_part);
        remove_quietly(&exe_part);
    };

    // 1) A assinatura primeiro: sem ela a Release ainda não foi assinada e nada é baixado.
    let status = dl.download(DOWNLOAD_HOST, &asset_path(version, &sig_name), &sig_part, MAX_SIGNATURE).inspect_err(|_| cleanup())?;
    if status != 200 {
        cleanup();
        return Err(status_error(status, UpdateError::NotSigned));
    }
    let signature = fs::read_to_string(&sig_part).map_err(|_| {
        cleanup();
        UpdateError::BadResponse
    })?;

    // 2) O instalador, como `.partial`.
    let status = dl.download(DOWNLOAD_HOST, &asset_path(version, &name), &exe_part, MAX_INSTALLER).inspect_err(|_| cleanup())?;
    if status != 200 {
        cleanup();
        return Err(status_error(status, UpdateError::NoInstaller));
    }

    // 3) Só depois de verificado ganha o nome final.
    let verified = fs::File::open(&exe_part)
        .map_err(|_| UpdateError::Disk)
        .and_then(|f| sha256_hex(f).map_err(UpdateError::Verify))
        .and_then(|sha| verify_release(keys, version, &sha, &signature).map_err(UpdateError::Verify));
    if let Err(e) = verified {
        cleanup();
        return Err(e);
    }
    let final_exe = dir.join(&name);
    let final_sig = dir.join(&sig_name);
    let moved = fs::rename(&exe_part, &final_exe).and_then(|_| fs::rename(&sig_part, &final_sig));
    if moved.is_err() {
        cleanup();
        remove_quietly(&final_exe);
        return Err(UpdateError::Disk);
    }
    Ok(final_exe)
}

/// Confere de novo um instalador já preparado, imediatamente antes de executá-lo.
pub fn reverify(version: &Version, dir: &Path) -> Result<PathBuf, UpdateError> {
    let keys = trusted_keys();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    reverify_with(&keys, version, dir)
}

pub(crate) fn reverify_with(keys: &[&str], version: &Version, dir: &Path) -> Result<PathBuf, UpdateError> {
    let name = installer_name(version);
    let exe = dir.join(&name);
    let signature = fs::read_to_string(dir.join(format!("{name}.sig"))).map_err(|_| UpdateError::Disk)?;
    let sha = fs::File::open(&exe).map_err(|_| UpdateError::Disk).and_then(|f| sha256_hex(f).map_err(UpdateError::Verify))?;
    verify_release(keys, version, &sha, &signature).map_err(UpdateError::Verify)?;
    Ok(exe)
}

/// Argumentos com que o instalador é aberto: progresso sem perguntas, modo atualização e reabrir o app.
pub const INSTALLER_ARGS: [&str; 3] = ["/P", "/UPDATE", "/R"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::signed_message;
    use ed25519_dalek::{Signer, SigningKey};
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[3u8; 32])
    }

    fn pub_hex(k: &SigningKey) -> String {
        k.verifying_key().as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    fn sig_for(k: &SigningKey, version: &str, file: &[u8]) -> String {
        let sha = sha256_hex(file).unwrap();
        let sig = k.sign(&signed_message(&v(version), &sha));
        sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Servidor simulado: caminho -> (status, conteúdo).
    struct Fake {
        files: BTreeMap<String, (u16, Vec<u8>)>,
        asked: RefCell<Vec<(String, String, u64)>>,
        fail_with: Option<UpdateError>,
    }

    impl Fake {
        fn release(version: &str, exe: &[u8], sig: &str) -> Fake {
            let ver = v(version);
            let name = installer_name(&ver);
            let mut files = BTreeMap::new();
            files.insert(asset_path(&ver, &name), (200, exe.to_vec()));
            files.insert(asset_path(&ver, &format!("{name}.sig")), (200, sig.as_bytes().to_vec()));
            Fake { files, asked: RefCell::new(vec![]), fail_with: None }
        }
    }

    impl Downloader for Fake {
        fn download(&self, host: &str, path: &str, dest: &Path, max: u64) -> Result<u16, UpdateError> {
            self.asked.borrow_mut().push((host.into(), path.into(), max));
            if let Some(e) = self.fail_with {
                return Err(e);
            }
            match self.files.get(path) {
                None => Ok(404),
                Some((200, body)) => {
                    if body.len() as u64 > max {
                        return Err(UpdateError::TooLarge);
                    }
                    fs::write(dest, body).map_err(|_| UpdateError::Disk)?;
                    Ok(200)
                }
                Some((status, _)) => Ok(*status),
            }
        }
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    const EXE: &[u8] = b"synthetic installer bytes";

    #[test]
    fn a_signed_release_is_downloaded_verified_and_only_then_named_as_an_installer() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        let path = prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f).unwrap();
        assert_eq!(path, dir.path().join("Developer-Black-Box_0.2.0_x64-setup.exe"));
        assert_eq!(fs::read(&path).unwrap(), EXE);
        assert_eq!(files_in(dir.path()), ["Developer-Black-Box_0.2.0_x64-setup.exe", "Developer-Black-Box_0.2.0_x64-setup.exe.sig"]);
        assert!(reverify_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path()).is_ok());
    }

    #[test]
    fn asks_only_github_with_paths_built_from_the_validated_version() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f).unwrap();
        let asked = f.asked.borrow();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[0],
            (
                "github.com".into(),
                "/GabrielVianaNunes/Developer-Black-Box/releases/download/v0.2.0/Developer-Black-Box_0.2.0_x64-setup.exe.sig".into(),
                MAX_SIGNATURE
            )
        );
        assert_eq!(
            asked[1],
            (
                "github.com".into(),
                "/GabrielVianaNunes/Developer-Black-Box/releases/download/v0.2.0/Developer-Black-Box_0.2.0_x64-setup.exe".into(),
                MAX_INSTALLER
            )
        );
    }

    #[test]
    fn an_unsigned_release_downloads_nothing_else() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let mut f = Fake::release("0.2.0", EXE, "");
        f.files.remove(&asset_path(&v("0.2.0"), "Developer-Black-Box_0.2.0_x64-setup.exe.sig"));
        let r = prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f);
        assert_eq!(r, Err(UpdateError::NotSigned));
        assert_eq!(f.asked.borrow().len(), 1, "the installer is not even requested");
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_missing_installer_or_unexpected_status_is_reported_and_cleaned_up() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let mut f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        f.files.remove(&asset_path(&v("0.2.0"), "Developer-Black-Box_0.2.0_x64-setup.exe"));
        assert_eq!(prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f), Err(UpdateError::NoInstaller));
        assert!(files_in(dir.path()).is_empty());
        let mut g = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        g.files.insert(asset_path(&v("0.2.0"), "Developer-Black-Box_0.2.0_x64-setup.exe"), (503, vec![]));
        assert_eq!(prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &g), Err(UpdateError::Status(503)));
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_tampered_installer_is_deleted_and_never_gets_the_installer_name() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let f = Fake::release("0.2.0", b"EVIL installer bytes", &sig_for(&key(), "0.2.0", EXE));
        let r = prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f);
        assert_eq!(r, Err(UpdateError::Verify(crate::VerifyError::Mismatch)));
        assert!(files_in(dir.path()).is_empty(), "{:?}", files_in(dir.path()));
    }

    #[test]
    fn a_signature_from_another_key_or_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let f = Fake::release("0.2.0", EXE, &sig_for(&attacker, "0.2.0", EXE));
        assert_eq!(prepare_with(&[&pub_hex(&key())], &v("0.2.0"), dir.path(), &f), Err(UpdateError::Verify(crate::VerifyError::Mismatch)));
        // Um instalador antigo e legítimo (assinado como 0.1.0) não passa por 0.2.0.
        let replay = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.1.0", EXE));
        assert_eq!(
            prepare_with(&[&pub_hex(&key())], &v("0.2.0"), dir.path(), &replay),
            Err(UpdateError::Verify(crate::VerifyError::Mismatch))
        );
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn a_malformed_signature_file_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["", "not hex", "<html>404</html>"] {
            let f = Fake::release("0.2.0", EXE, bad);
            assert_eq!(
                prepare_with(&[&pub_hex(&key())], &v("0.2.0"), dir.path(), &f),
                Err(UpdateError::Verify(crate::VerifyError::BadSignatureFormat)),
                "{bad:?}"
            );
            assert!(files_in(dir.path()).is_empty());
        }
    }

    #[test]
    fn oversized_downloads_and_transport_errors_leave_nothing_behind() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let mut f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        f.files.insert(asset_path(&v("0.2.0"), "Developer-Black-Box_0.2.0_x64-setup.exe"), (200, vec![0u8; (MAX_INSTALLER + 1) as usize]));
        assert_eq!(prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f), Err(UpdateError::TooLarge));
        assert!(files_in(dir.path()).is_empty());
        let mut net = Fake::release("0.2.0", EXE, "x");
        net.fail_with = Some(UpdateError::Network);
        assert_eq!(prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &net), Err(UpdateError::Network));
        assert!(files_in(dir.path()).is_empty());
    }

    #[test]
    fn leftovers_from_earlier_attempts_are_removed_first() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        fs::write(dir.path().join("Developer-Black-Box_0.1.5_x64-setup.exe"), b"old").unwrap();
        fs::write(dir.path().join("half.partial"), b"half").unwrap();
        let f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f).unwrap();
        assert_eq!(files_in(dir.path()), ["Developer-Black-Box_0.2.0_x64-setup.exe", "Developer-Black-Box_0.2.0_x64-setup.exe.sig"]);
    }

    #[test]
    fn reverify_catches_a_file_swapped_after_the_download() {
        let (k, dir) = (key(), tempfile::tempdir().unwrap());
        let f = Fake::release("0.2.0", EXE, &sig_for(&key(), "0.2.0", EXE));
        let path = prepare_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path(), &f).unwrap();
        fs::write(&path, b"swapped after verification").unwrap();
        assert_eq!(reverify_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path()), Err(UpdateError::Verify(crate::VerifyError::Mismatch)));
        fs::remove_file(&path).unwrap();
        assert_eq!(reverify_with(&[&pub_hex(&k)], &v("0.2.0"), dir.path()), Err(UpdateError::Disk));
    }

    #[test]
    fn the_installer_is_opened_in_progress_only_update_mode_and_relaunches_the_app() {
        assert_eq!(INSTALLER_ARGS, ["/P", "/UPDATE", "/R"]);
    }

    #[test]
    fn names_follow_what_the_release_workflow_publishes() {
        assert_eq!(installer_name(&v("1.2.3")), "Developer-Black-Box_1.2.3_x64-setup.exe");
        assert_eq!(installer_name(&v("1.2.3-rc.1")), "Developer-Black-Box_1.2.3-rc.1_x64-setup.exe");
    }
}
