//! A verificação de atualizações é a única rede do app. Estes testes garantem que ela continua
//! isolada, mínima e sem bibliotecas de rede de terceiros. Somente dados sintéticos.

use std::fs;
use std::path::Path;

/// Pacotes do grafo de dependências (build + normais) de `pkg` no alvo Windows.
fn windows_closure(pkg: &str) -> std::collections::BTreeSet<String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args(["tree", "-p", pkg, "--target", "x86_64-pc-windows-msvc", "--prefix", "none", "-e", "normal,build", "--offline", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("run cargo tree");
    assert!(out.status.success(), "cargo tree failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split_whitespace().next()).map(str::to_owned).collect()
}

const HTTP_TLS_CLIENTS: [&str; 9] =
    ["reqwest", "hyper", "ureq", "curl", "isahc", "surf", "native-tls", "openssl", "rustls"];
const SOCKET_STACKS: [&str; 4] = ["tokio", "mio", "socket2", "h2"];

// A verificação de atualizações é a única rede do app, isolada em `bb-update`: o núcleo não depende dela
// e ela própria não traz nenhum cliente HTTP/TLS nem pilha de sockets (usa o WinHTTP do Windows).
#[test]
fn update_check_is_isolated_and_brings_no_network_library() {
    for pkg in ["bb-core", "bb-recorder", "bb-collector", "bb-store", "bb-query", "bb-engine", "bb-tray"] {
        assert!(!windows_closure(pkg).contains("bb-update"), "{pkg} must not depend on the update checker");
    }
    let deps = windows_closure("bb-update");
    assert!(deps.contains("bb-update"));
    for banned in HTTP_TLS_CLIENTS.iter().chain(SOCKET_STACKS.iter()) {
        assert!(!deps.contains(*banned), "bb-update depends on network library: {banned}");
    }
}

// Nenhum outro código do app abre conexões: só `bb-update` pode usar WinHTTP ou sockets.
#[test]
fn no_network_calls_outside_the_update_crate() {
    const NETWORK_API: [&str; 8] =
        ["Win32::Networking", "Win32_Networking", "std::net", "TcpStream", "UdpSocket", "WinInet", "InternetOpen", "URLDownloadToFile"];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut dirs: Vec<std::path::PathBuf> = fs::read_dir(root.join("crates"))
        .unwrap()
        .map(|e| e.unwrap().path().join("src"))
        .filter(|p| !p.ends_with("bb-update/src"))
        .collect();
    dirs.push(root.join("src-tauri/src"));
    let mut scanned = 0;
    for dir in dirs {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                scanned += 1;
                for api in NETWORK_API {
                    assert!(!text.contains(api), "{} uses network API `{api}`", path.display());
                }
            }
        }
    }
    assert!(scanned > 10, "scan must actually cover the source files ({scanned})");
}

// O destino é fixo e HTTPS: nada que o usuário, o disco ou a rede digitem muda para onde o app conecta.
#[test]
fn update_check_talks_only_to_the_projects_github_releases() {
    assert_eq!(bb_update::HOST, "api.github.com");
    assert_eq!(bb_update::latest_path(), "/repos/GabrielVianaNunes/Developer-Black-Box/releases/latest");
    let url = bb_update::release_url(&bb_update::Version::parse("1.2.3").unwrap());
    assert_eq!(url, "https://github.com/GabrielVianaNunes/Developer-Black-Box/releases/tag/v1.2.3");
}

