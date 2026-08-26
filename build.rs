fn main() {
    // Bundle des traductions gettext (.po) trouvées sous lang/<locale>/LC_MESSAGES/
    // ports-launcher.po -- slint-build les parse et les embarque directement dans
    // le binaire, aucun outillage gettext externe (msgfmt) requis. Contexte par
    // défaut désactivé : sans lui, chaque @tr("...") marqué dans les .slint devrait
    // porter le nom du composant comme msgctxt implicite, ce qui obligerait les .po
    // écrits à la main à dupliquer cette info -- inutile ici, aucune chaîne
    // n'entre en collision entre composants.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app-window.slint", config).expect("échec de la compilation Slint");

    // Date du jour (UTC) au format des tags de release GitHub (MM/DD/YY,
    // ex. "08/19/26") -- exposée comme APP_VERSION (voir core::version) pour
    // ne plus jamais avoir à la taper à la main dans le code source. Publier
    // le tag GitHub le même jour calendaire UTC que cette compilation, sinon
    // le build embarqué et le tag publié divergent d'un jour (voir le
    // commentaire de version.rs pour le détail du problème que ça évite).
    let build_date = chrono::Utc::now().format("%m/%d/%y").to_string();
    println!("cargo:rustc-env=APP_BUILD_DATE={build_date}");

    // Copie 7z.exe/7z.dll à côté de l'exécutable produit -- `installer::extract`
    // les cherche au runtime dans le dossier de l'exe (voir sevenzip_exe_path),
    // jamais liés au binaire (processus externe, pas un crate). Copiés ici
    // pour que `cargo build`/`cargo test` les rendent disponibles sans étape
    // manuelle, y compris pour les tests (`cargo test` tourne depuis
    // target/debug, jamais couvert par Start.bat qui ne visait que
    // target/release).
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR absent");
    if let Some(profile_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) {
        for name in ["7z.exe", "7z.dll"] {
            let _ = std::fs::copy(name, profile_dir.join(name));
            println!("cargo:rerun-if-changed={name}");
        }
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("Icon.ico")
            // Métadonnées de version Windows -- un exécutable non signé sans
            // aucune métadonnée (nom d'origine, description...) est un des
            // signaux que certains moteurs heuristiques/ML utilisent pour
            // juger un binaire "suspect", en plus de sa faible diffusion --
            // d'autant plus pertinent ici que ce launcher télécharge et
            // exécute des binaires tiers (voir MAGI Launcher, même choix).
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
