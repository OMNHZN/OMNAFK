use sha2::Digest;
use std::io::Write;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(omnafk_embed_payload)");
    println!("cargo:rerun-if-env-changed=OMNAFK_PAYLOAD_EXE");
    if let Some(payload_path) = std::env::var_os("OMNAFK_PAYLOAD_EXE") {
        println!("cargo:rerun-if-changed={}", payload_path.to_string_lossy());
        let raw = std::fs::read(&payload_path).expect("failed to read OMNAFK_PAYLOAD_EXE");
        let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR not set");
        let gz_path = std::path::Path::new(&out_dir).join("omnafk-payload.gz");

        let file = std::fs::File::create(&gz_path).expect("failed to create payload archive");
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
        encoder
            .write_all(&raw)
            .and_then(|()| encoder.finish().map(|_| ()))
            .expect("failed to compress payload");

        let hash = sha2::Sha256::digest(&raw);
        let hash_hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
        println!("cargo:rustc-env=OMNAFK_PAYLOAD_SHA256={hash_hex}");

        println!("cargo:rustc-cfg=omnafk_embed_payload");
        println!("cargo:rustc-env=OMNAFK_PAYLOAD_GZ={}", gz_path.display());
        println!("cargo:rustc-env=OMNAFK_PAYLOAD_RAW_LEN={}", raw.len());
    }

    // Optional bundled ViGEmBus driver installer (for the Gamepad nudge action).
    // Embedded only when OMNAFK_VIGEM_EXE points at the redistributable.
    println!("cargo:rustc-check-cfg=cfg(omnafk_embed_vigem)");
    println!("cargo:rerun-if-env-changed=OMNAFK_VIGEM_EXE");
    if let Some(vigem_path) = std::env::var_os("OMNAFK_VIGEM_EXE") {
        println!("cargo:rerun-if-changed={}", vigem_path.to_string_lossy());
        let raw = std::fs::read(&vigem_path).expect("failed to read OMNAFK_VIGEM_EXE");
        let out_dir = std::env::var_os("OUT_DIR").expect("OUT_DIR not set");
        let gz_path = std::path::Path::new(&out_dir).join("vigem-payload.gz");

        let file = std::fs::File::create(&gz_path).expect("failed to create vigem archive");
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::best());
        encoder
            .write_all(&raw)
            .and_then(|()| encoder.finish().map(|_| ()))
            .expect("failed to compress vigem installer");

        println!("cargo:rustc-cfg=omnafk_embed_vigem");
        println!("cargo:rustc-env=OMNAFK_VIGEM_GZ={}", gz_path.display());
        println!("cargo:rustc-env=OMNAFK_VIGEM_RAW_LEN={}", raw.len());
    }

    tauri_build::build();
}
