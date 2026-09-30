fn main() {
    // O ícone do executável vem do mesmo renderizador da bandeja (luz cinza = neutro),
    // então não há binário versionado e os dois nunca divergem.
    let ico = bb_tray::icon::encode_ico(&[16, 24, 32, 48, 64, 256], bb_tray::Light::Gray);
    std::fs::create_dir_all("icons").expect("create icons dir");
    std::fs::write("icons/icon.ico", ico).expect("write icon.ico");
    tauri_build::build();
}
