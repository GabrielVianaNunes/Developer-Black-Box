fn main() {
    // O ícone do executável vem do mesmo renderizador da bandeja, só que sem a luz: o Windows mostra esse ícone
    // (ou o do atalho) no botão da barra de tarefas e não o troca; a cor do estado vai no selo do botão.
    let ico = bb_tray::icon::encode_ico_cube(&[16, 24, 32, 48, 64, 256]);
    std::fs::create_dir_all("icons").expect("create icons dir");
    std::fs::write("icons/icon.ico", ico).expect("write icon.ico");
    tauri_build::build();
}
