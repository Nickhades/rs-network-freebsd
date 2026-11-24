//! Fixed application themes, independent of the desktop GTK theme.
//!
//! netcore ships exactly two palettes. Dark is the default; the in-window
//! toggle switches to the light palette. The system font *family* is detected
//! and used everywhere with a bold weight and high-quality antialiasing.

use std::cell::Cell;
use std::rc::Rc;

use gtk::gdk;
use gtk4 as gtk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl ThemeMode {
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "深色",
            Self::Light => "浅色",
        }
    }

    #[must_use]
    pub const fn action_label(self) -> &'static str {
        match self {
            Self::Dark => "切换到浅色模式",
            Self::Light => "切换到深色模式",
        }
    }
}

#[derive(Clone)]
pub struct Themer {
    provider: gtk::CssProvider,
    font_family: String,
    mode: Rc<Cell<ThemeMode>>,
}

impl Themer {
    pub fn new(display: &gdk::Display, initial: ThemeMode) -> Self {
        let font_family = detect_system_font_family();
        let provider = gtk::CssProvider::new();
        let themer = Self {
            provider,
            font_family,
            mode: Rc::new(Cell::new(initial)),
        };
        themer.apply();
        gtk::style_context_add_provider_for_display(
            display,
            &themer.provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER + 10,
        );
        themer
    }

    #[must_use]
    pub fn font_family(&self) -> &str {
        &self.font_family
    }

    pub fn toggle(&self) -> ThemeMode {
        let mode = self.mode.get().toggled();
        self.mode.set(mode);
        self.apply();
        mode
    }

    pub fn apply(&self) {
        let palette = Palette::for_mode(self.mode.get());
        let css = css(&palette, &self.font_family);
        self.provider.load_from_data(&css);
    }
}

fn detect_system_font_family() -> String {
    let settings = gtk::Settings::default();

    if let Some(settings) = &settings {
        // Keep colour decisions in our own CSS, but request smooth grayscale
        // antialiasing and light hinting for the app's text rendering.
        settings.set_gtk_xft_antialias(1);
        settings.set_gtk_xft_hinting(1);
        settings.set_gtk_xft_hintstyle(Some("hintslight"));
    }

    let font_name = settings
        .as_ref()
        .and_then(|settings| settings.gtk_font_name())
        .map_or_else(|| "Sans 11".to_owned(), |name| name.to_string());
    let description = gtk::pango::FontDescription::from_string(&font_name);
    description
        .family()
        .filter(|family| !family.trim().is_empty())
        .map_or_else(|| "Sans".to_owned(), |family| family.to_string())
}

struct Palette {
    bg: &'static str,
    bg_alt: &'static str,
    surface: &'static str,
    surface_hover: &'static str,
    surface_active: &'static str,
    border: &'static str,
    text: &'static str,
    dim: &'static str,
    accent: &'static str,
    accent_soft: &'static str,
    on_accent: &'static str,
    success: &'static str,
    success_soft: &'static str,
    error: &'static str,
    error_soft: &'static str,
    shadow: &'static str,
    text_shadow: &'static str,
}

impl Palette {
    const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                bg: "#0E141B",
                bg_alt: "#131C25",
                surface: "#18222D",
                surface_hover: "#202D3A",
                surface_active: "#283747",
                border: "#2E3D4D",
                text: "#F3F7FB",
                dim: "#9BAEBD",
                accent: "#53C997",
                accent_soft: "rgba(83,201,151,0.15)",
                on_accent: "#07130E",
                success: "#53C997",
                success_soft: "rgba(83,201,151,0.13)",
                error: "#FF7A7A",
                error_soft: "rgba(255,122,122,0.13)",
                shadow: "rgba(0,0,0,0.45)",
                text_shadow: "rgba(0,0,0,0.32)",
            },
            ThemeMode::Light => Self {
                bg: "#EEF3F8",
                bg_alt: "#E4EBF2",
                surface: "#FFFFFF",
                surface_hover: "#F1F6FA",
                surface_active: "#E2EBF3",
                border: "#C8D5E1",
                text: "#13202B",
                dim: "#5B6D7D",
                accent: "#16745E",
                accent_soft: "rgba(22,116,94,0.12)",
                on_accent: "#FFFFFF",
                success: "#16745E",
                success_soft: "rgba(22,116,94,0.12)",
                error: "#B93838",
                error_soft: "rgba(185,56,56,0.12)",
                shadow: "rgba(31,46,61,0.16)",
                text_shadow: "rgba(255,255,255,0.42)",
            },
        }
    }
}

fn css(palette: &Palette, font_family: &str) -> String {
    let font_family = font_family.replace(['"', '\\'], "");
    let font_family = font_family.replace('\n', " ");

    let template = r#"
@define-color nc_bg __BG__;
@define-color nc_bg_alt __BG_ALT__;
@define-color nc_surface __SURFACE__;
@define-color nc_surface_hover __SURFACE_HOVER__;
@define-color nc_surface_active __SURFACE_ACTIVE__;
@define-color nc_border __BORDER__;
@define-color nc_text __TEXT__;
@define-color nc_dim __DIM__;
@define-color nc_accent __ACCENT__;
@define-color nc_accent_soft __ACCENT_SOFT__;
@define-color nc_on_accent __ON_ACCENT__;
@define-color nc_success __SUCCESS__;
@define-color nc_success_soft __SUCCESS_SOFT__;
@define-color nc_error __ERROR__;
@define-color nc_error_soft __ERROR_SOFT__;
@define-color nc_shadow __SHADOW__;

* {
    font-family: "__FONT__", sans-serif;
    font-weight: 700;
}

window,
window.background,
.background {
    background-color: @nc_bg;
    color: @nc_text;
}

label {
    color: @nc_text;
    text-shadow: 0 1px 1px __TEXT_SHADOW__;
}

.dim-label,
label.dim-label,
.caption {
    color: @nc_dim;
    opacity: 1;
}

.success,
label.success {
    color: @nc_success;
}

.error,
label.error {
    color: @nc_error;
}

headerbar {
    background-color: @nc_bg;
    background-image: none;
    color: @nc_text;
    border-bottom: 1px solid @nc_border;
    box-shadow: none;
    min-height: 38px;
    padding: 0 8px;
}

headerbar:backdrop {
    background-color: @nc_bg;
    color: @nc_text;
}

.netcore-brand {
    padding-left: 2px;
}

.netcore-app-logo {
    margin: 0 2px 0 2px;
}

.netcore-brand-label {
    color: @nc_accent;
    font-size: 12px;
    font-weight: 800;
    text-shadow: none;
}

headerbar .title {
    color: @nc_text;
    font-weight: 800;
    font-size: 15px;
}

headerbar button,
.netcore-icon-button {
    background: transparent;
    background-image: none;
    color: @nc_dim;
    border: none;
    border-radius: 6px;
    box-shadow: none;
    padding: 3px;
    min-width: 26px;
    min-height: 26px;
}

headerbar button:hover,
.netcore-icon-button:hover {
    background: @nc_surface_hover;
    color: @nc_text;
}

headerbar button:active,
.netcore-icon-button:active {
    background: @nc_surface_active;
    color: @nc_text;
}

.netcore-theme-button {
    padding: 3px 7px;
    min-width: 0;
}

.netcore-theme-button label {
    color: @nc_dim;
    font-size: 11px;
    text-shadow: none;
}

.netcore-theme-button:hover label {
    color: @nc_text;
}

.netcore-window-controls button:hover {
    background: @nc_surface_hover;
}

.netcore-window-controls button.netcore-close:hover {
    background: @nc_error;
    color: @nc_text;
}

button {
    background-color: @nc_surface;
    background-image: none;
    color: @nc_text;
    border: 1px solid @nc_border;
    border-radius: 7px;
    box-shadow: none;
    padding: 3px 9px;
    min-height: 24px;
}

button:hover {
    background-color: @nc_surface_hover;
    border-color: @nc_border;
}

button:active,
button:checked {
    background-color: @nc_surface_active;
}

button.flat {
    background-color: transparent;
    border-color: transparent;
}

button.flat:hover {
    background-color: @nc_surface_hover;
}

button.suggested-action {
    background-color: @nc_accent;
    border-color: @nc_accent;
    color: @nc_on_accent;
}

button.suggested-action:hover {
    background-color: @nc_accent;
}

button.suggested-action label {
    color: @nc_on_accent;
    text-shadow: none;
}

.card,
.netcore-hero,
.netcore-device {
    background-color: @nc_surface;
    background-image: none;
    border: 1px solid @nc_border;
    border-radius: 10px;
    box-shadow: 0 6px 18px @nc_shadow;
}

.netcore-section-title {
    color: @nc_dim;
    font-size: 12px;
    font-weight: 800;
}

.heading {
    color: @nc_text;
    font-size: 13px;
    font-weight: 800;
}

.title-3 {
    color: @nc_text;
    font-size: 18px;
    font-weight: 800;
}

.netcore-network-row {
    background-color: transparent;
    background-image: none;
    border: 1px solid transparent;
    border-radius: 7px;
    box-shadow: none;
    color: @nc_text;
    padding: 2px 6px;
    min-height: 0;
}

.netcore-network-row:hover {
    background-color: @nc_surface_hover;
    border-color: @nc_border;
}

.netcore-network-row label {
    color: @nc_text;
}

.netcore-network-row .caption {
    color: @nc_dim;
}

.netcore-current,
.netcore-current:hover {
    background-color: @nc_accent_soft;
    border-color: @nc_accent;
}

.netcore-current .netcore-icon,
.netcore-current .dim-label {
    color: @nc_accent;
    opacity: 1;
}

.netcore-current label {
    color: @nc_text;
}

.netcore-current label.success {
    color: @nc_success;
}

entry,
passwordentry {
    background-color: @nc_bg_alt;
    background-image: none;
    color: @nc_text;
    border: 1px solid @nc_border;
    border-radius: 7px;
    box-shadow: none;
    padding: 5px 8px;
}

entry:focus,
entry:focus-within,
passwordentry:focus,
passwordentry:focus-within {
    border-color: @nc_accent;
    box-shadow: inset 0 0 0 1px @nc_accent;
    outline-color: @nc_accent;
    outline-width: 1px;
    outline-offset: -2px;
}

entry > text > placeholder,
passwordentry > text > placeholder {
    color: @nc_dim;
}

button:focus,
switch:focus,
switch:focus-visible {
    outline-color: @nc_accent;
}

entry selection {
    background-color: @nc_accent;
    color: @nc_on_accent;
}

switch {
    background-color: @nc_border;
    background-image: none;
    border: 1px solid @nc_border;
    border-radius: 999px;
    box-shadow: none;
    min-width: 36px;
    min-height: 20px;
}

switch:checked {
    background-color: @nc_accent;
    border-color: @nc_accent;
}

switch slider {
    background-color: @nc_text;
    background-image: none;
    border: none;
    border-radius: 999px;
    box-shadow: 0 1px 2px @nc_shadow;
    min-width: 15px;
    min-height: 15px;
    margin: 2px;
}

switch:checked slider {
    background-color: @nc_on_accent;
}

tooltip {
    background-color: @nc_surface;
    color: @nc_text;
    border: 1px solid @nc_border;
    border-radius: 6px;
    box-shadow: 0 4px 16px @nc_shadow;
}

tooltip label {
    color: @nc_text;
    text-shadow: none;
}

scrollbar {
    background-color: transparent;
    border: none;
}

scrollbar slider {
    background-color: @nc_border;
    border: none;
    border-radius: 999px;
    min-width: 5px;
    min-height: 24px;
}

scrollbar slider:hover {
    background-color: @nc_dim;
}

separator {
    background-color: @nc_border;
    min-height: 1px;
    min-width: 1px;
}

.netcore-banner {
    background-color: @nc_surface;
    border: 1px solid @nc_border;
    border-radius: 8px;
    box-shadow: none;
}

.netcore-banner-error {
    background-color: @nc_error_soft;
    border-color: @nc_error;
}

.netcore-banner-success {
    background-color: @nc_success_soft;
    border-color: @nc_success;
}

.netcore-footer {
    color: @nc_dim;
    padding: 1px 14px 5px 14px;
}

.netcore-footer label {
    color: @nc_dim;
    font-size: 11px;
    text-shadow: none;
}

.netcore-password-row entry {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
}

.netcore-password-row button {
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    min-width: 30px;
}

.netcore-icon {
    background: transparent;
    color: @nc_text;
}

.netcore-icon-dim {
    color: @nc_dim;
}

window.dialog,
window {
    background-color: @nc_bg;
    color: @nc_text;
}
"#;

    template
        .replace("__BG_ALT__", palette.bg_alt)
        .replace("__BG__", palette.bg)
        .replace("__SURFACE_HOVER__", palette.surface_hover)
        .replace("__SURFACE_ACTIVE__", palette.surface_active)
        .replace("__SURFACE__", palette.surface)
        .replace("__BORDER__", palette.border)
        .replace("__TEXT__", palette.text)
        .replace("__DIM__", palette.dim)
        .replace("__ACCENT_SOFT__", palette.accent_soft)
        .replace("__ACCENT__", palette.accent)
        .replace("__ON_ACCENT__", palette.on_accent)
        .replace("__SUCCESS_SOFT__", palette.success_soft)
        .replace("__SUCCESS__", palette.success)
        .replace("__ERROR_SOFT__", palette.error_soft)
        .replace("__ERROR__", palette.error)
        .replace("__SHADOW__", palette.shadow)
        .replace("__TEXT_SHADOW__", palette.text_shadow)
        .replace("__FONT__", &font_family)
}
