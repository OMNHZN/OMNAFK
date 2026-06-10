fn main() {
    println!("cargo:rustc-check-cfg=cfg(omnafk_embed_payload)");
    println!("cargo:rerun-if-env-changed=OMNAFK_PAYLOAD_EXE");
    if std::env::var_os("OMNAFK_PAYLOAD_EXE").is_some() {
        println!("cargo:rustc-cfg=omnafk_embed_payload");
    }
    tauri_build::build();
}
