fn main() {
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app-window.slint", config).expect("échec de la compilation Slint");

    let build_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    println!("cargo:rustc-env=APP_BUILD_DATE={build_date}");

    let is_windows_target = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let sevenzip_files: &[&str] = if is_windows_target { &["7z.exe", "7z.dll"] } else { &["7zzs"] };
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR absent");
    if let Some(profile_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) {
        for name in sevenzip_files {
            let dest = profile_dir.join(name);
            if std::fs::copy(name, &dest).is_ok() && !is_windows_target {
                #[cfg(target_os = "linux")]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755));
                }
            }
            println!("cargo:rerun-if-changed={name}");
        }
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("Icon.ico")
            .set("FileDescription", "Ports Launcher")
            .set("ProductName", "Ports Launcher")
            .set("OriginalFilename", "ports_launcher.exe")
            .set("InternalName", "ports_launcher")
            .set("CompanyName", "Nyaldee")
            .set("LegalCopyright", "Copyright © 2026 Nyaldee")
            .compile()
            .expect("échec de l'embarquement de l'icône");
    }
}
