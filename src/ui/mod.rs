pub mod dialog_geometry;
pub mod font_metrics;
pub mod font_sizing;
pub mod gamepad_router;
pub mod geometry;
#[cfg(target_os = "linux")]
pub mod linux_chrome;
#[cfg(test)]
mod slint_layout_lint;
pub mod theme;
#[cfg(target_os = "windows")]
pub mod windows_chrome;

/// Détail natif de fenêtre (focus/modal, icône, DPI, glissé...), un module
/// par OS -- voir `windows_chrome`/`linux_chrome`, qui exposent
/// délibérément les mêmes noms de fonction pour que tout le reste du code
/// n'ait qu'un seul alias à importer, jamais de `#[cfg]` par site d'appel.
#[cfg(target_os = "windows")]
pub use windows_chrome as chrome;
#[cfg(target_os = "linux")]
pub use linux_chrome as chrome;
