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

#[cfg(target_os = "windows")]
pub use windows_chrome as chrome;
#[cfg(target_os = "linux")]
pub use linux_chrome as chrome;
