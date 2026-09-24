use std::path::Path;

fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app-window.slint", config).expect("Slint compilation failed");

    println!("cargo:rustc-env=APP_BUILD_DATE={}", chrono::Utc::now().format("%Y-%m-%d"));

    let is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    copy_sevenzip(is_windows);

    if is_windows {
        winresource::WindowsResource::new()
            .set_icon("Icon.ico")
            .set("FileDescription", "Ports Launcher")
            .set("ProductName", "Ports Launcher")
            .set("OriginalFilename", "ports_launcher.exe")
            .set("InternalName", "ports_launcher")
            .set("CompanyName", "Nyaldee")
            .set("LegalCopyright", "Copyright © 2026 Nyaldee")
            .compile()
            .expect("failed to embed Windows resources");
    }
}

fn copy_sevenzip(is_windows: bool) {
    let files: &[&str] = if is_windows { &["7z.exe", "7z.dll"] } else { &["7zzs"] };
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is not set");
    let Some(profile_dir) = Path::new(&out_dir).ancestors().nth(3) else { return };
    for name in files {
        println!("cargo:rerun-if-changed={name}");
        let dest = profile_dir.join(name);
        if std::fs::copy(name, &dest).is_err() {
            continue;
        }
        #[cfg(unix)]
        if !is_windows {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
        }
    }
}
