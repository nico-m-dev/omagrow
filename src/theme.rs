//! Follows the active Omarchy theme.
//!
//! Omarchy stages the current palette at `~/.local/state/omarchy/current/theme/colors.toml`.
//! We read it, map it onto libadwaita's named colours and watch the directory so a theme
//! switch is picked up while the app is open.

use std::fs;
use std::path::PathBuf;
use std::sync::Once;

use gtk::gdk;
use gtk::gio;
use gtk::prelude::*;
use libadwaita as adw;

static INSTALL: Once = Once::new();

const FALLBACK_BG: &str = "#1e1e1e";
const FALLBACK_FG: &str = "#ffffff";
const FALLBACK_ACCENT: &str = "#3584e4";
const FALLBACK_RED: &str = "#e01b24";
const FALLBACK_GREEN: &str = "#2ec27e";
const FALLBACK_NIGHT: &str = "#62a0ea";

pub fn install() {
    INSTALL.call_once(|| {
        let Some(display) = gdk::Display::default() else {
            return;
        };

        let provider = gtk::CssProvider::new();
        apply(&provider);
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        watch(provider);
    });
}

fn apply(provider: &gtk::CssProvider) {
    let palette = Palette::load();
    provider.load_from_string(&palette.to_css());
    adw::StyleManager::default().set_color_scheme(if palette.is_dark() {
        adw::ColorScheme::ForceDark
    } else {
        adw::ColorScheme::ForceLight
    });
}

fn watch(provider: gtk::CssProvider) {
    let Some(dir) = state_current_dir() else {
        return;
    };

    let monitor = gio::File::for_path(dir)
        .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE);

    let Ok(monitor) = monitor else {
        return;
    };

    monitor.connect_changed(move |_, _, _, _| apply(&provider));
    // The monitor has to outlive this call; it is created once per process.
    std::mem::forget(monitor);
}

fn state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME") {
        let dir = PathBuf::from(dir);
        if dir.is_absolute() {
            return dir;
        }
    }

    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
}

fn state_current_dir() -> Option<PathBuf> {
    let dir = state_dir().join("omarchy").join("current");
    dir.is_dir().then_some(dir)
}

struct Palette {
    background: String,
    foreground: String,
    accent: String,
    red: String,
    green: String,
    night: String,
}

impl Palette {
    fn load() -> Self {
        let mut palette = Self {
            background: FALLBACK_BG.to_string(),
            foreground: FALLBACK_FG.to_string(),
            accent: FALLBACK_ACCENT.to_string(),
            red: FALLBACK_RED.to_string(),
            green: FALLBACK_GREEN.to_string(),
            night: FALLBACK_NIGHT.to_string(),
        };

        let Some(dir) = state_current_dir() else {
            return palette;
        };

        let Ok(text) = fs::read_to_string(dir.join("theme").join("colors.toml")) else {
            return palette;
        };

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            let value = value.trim().trim_matches('"').trim();
            let Some(value) = normalize_hex(value) else {
                continue;
            };

            match key.trim() {
                "background" => palette.background = value.clone(),
                "foreground" => palette.foreground = value.clone(),
                "accent" => palette.accent = value.clone(),
                "red" | "color1" => palette.red = value.clone(),
                "green" | "color2" => palette.green = value.clone(),
                "blue" | "color4" => palette.night = value.clone(),
                _ => {}
            }
        }

        palette
    }

    fn is_dark(&self) -> bool {
        luminance(&self.background) < 0.5
    }

    fn to_css(&self) -> String {
        let bg = &self.background;
        let fg = &self.foreground;
        let accent = &self.accent;
        let sidebar = mix(bg, fg, 0.04);
        let raised = mix(bg, fg, 0.07);
        let line = rgba(fg, 0.12);
        let accent_fg = foreground_on(accent);
        let red = &self.red;
        let red_fg = foreground_on(red);
        let green = &self.green;
        let night = mix(&self.night, fg, 0.35);

        format!(
            "
@define-color window_bg_color {bg};
@define-color window_fg_color {fg};
@define-color view_bg_color {bg};
@define-color view_fg_color {fg};
@define-color headerbar_bg_color {bg};
@define-color headerbar_fg_color {fg};
@define-color headerbar_border_color {line};
@define-color headerbar_backdrop_color {bg};
@define-color headerbar_shade_color {line};
@define-color sidebar_bg_color {sidebar};
@define-color sidebar_fg_color {fg};
@define-color sidebar_backdrop_color {sidebar};
@define-color sidebar_shade_color {line};
@define-color secondary_sidebar_bg_color {sidebar};
@define-color secondary_sidebar_fg_color {fg};
@define-color card_bg_color {raised};
@define-color card_fg_color {fg};
@define-color dialog_bg_color {raised};
@define-color dialog_fg_color {fg};
@define-color popover_bg_color {raised};
@define-color popover_fg_color {fg};
@define-color accent_bg_color {accent};
@define-color accent_fg_color {accent_fg};
@define-color accent_color {accent};
@define-color destructive_bg_color {red};
@define-color destructive_fg_color {red_fg};
@define-color destructive_color {red};
@define-color omagrow_green {green};
@define-color omagrow_night {night};

.omagrow-sidebar {{
  background-color: {sidebar};
  border-right: 1px solid {line};
}}

.omagrow-logo {{
  font-family: \"iA Writer Quattro S\", \"Adwaita Sans\", sans-serif;
  font-size: 28px;
  font-weight: 700;
  letter-spacing: 0.01em;
  color: @accent_bg_color;
}}

.omagrow-logo-compact {{
  font-size: 20px;
}}

.omagrow-sidebar row.omagrow-active {{
  background-color: alpha(@window_fg_color, 0.09);
  border-radius: 8px;
}}

/* The three areas that hold a grow share the plant green; Help and Settings stay neutral. */
.omagrow-sidebar image.omagrow-nav-icon {{
  color: @omagrow_green;
}}

.omagrow-hero image.omagrow-plant-icon {{
  color: @omagrow_green;
}}

statuspage.omagrow-empty image {{
  color: @omagrow_green;
}}

.omagrow-profile-name {{
  font-size: 1.9em;
  font-weight: 800;
}}

.omagrow-chip {{
  border-radius: 999px;
  padding: 1px 8px;
  font-size: 0.78em;
  background-color: alpha(@window_fg_color, 0.08);
  color: alpha(@window_fg_color, 0.72);
}}

.omagrow-chip.estimated {{
  color: @accent_color;
}}

.omagrow-chip.confirmed {{
  color: @omagrow_green;
}}

.omagrow-chip.not-applicable {{
  color: alpha(@window_fg_color, 0.5);
}}

.omagrow-chip.unverified {{
  color: alpha(@window_fg_color, 0.55);
}}

.omagrow-chip.mixed {{
  color: @accent_color;
}}

.omagrow-chip.growing {{
  color: @omagrow_green;
  background-color: alpha(@omagrow_green, 0.12);
}}

.omagrow-chip.planned {{
  color: @accent_color;
  background-color: alpha(@accent_bg_color, 0.12);
}}

.omagrow-chip.finished {{
  color: alpha(@window_fg_color, 0.62);
  background-color: alpha(@window_fg_color, 0.07);
}}

.omagrow-thumb {{
  border-radius: 12px;
  background-color: alpha(@window_fg_color, 0.06);
}}

/* A cover sits against the card's top edge; the wrapper clips it to the card's corners. */
.omagrow-card-cover,
.omagrow-card-icon {{
  border-top-left-radius: 12px;
  border-top-right-radius: 12px;
}}

.omagrow-card-icon {{
  background-color: alpha(@window_fg_color, 0.06);
}}

.omagrow-card-icon image.omagrow-plant-icon {{
  color: @omagrow_green;
}}

.omagrow-thumb image {{
  color: @omagrow_green;
}}

.omagrow-section-title {{
  font-size: 0.82em;
  font-weight: 700;
  color: alpha(@window_fg_color, 0.62);
}}

.omagrow-timeline,
.omagrow-calendar-card {{
  padding: 16px;
}}

.omagrow-climate {{
  padding: 16px;
}}

.omagrow-climate-plot {{
  background-color: alpha(@window_fg_color, 0.035);
  border-radius: 12px;
  padding: 10px 12px 8px;
}}

.omagrow-climate-title {{
  font-size: 0.9em;
  font-weight: 700;
}}

.omagrow-legend {{
  font-size: 0.72em;
  color: alpha(@window_fg_color, 0.62);
}}

.omagrow-climate .linked button {{
  font-size: 0.82em;
  min-height: 24px;
  padding: 2px 10px;
}}

.omagrow-climate .omagrow-source-chip {{
  font-size: 0.78em;
  min-height: 24px;
  padding: 2px 9px;
}}

.omagrow-banner {{
  background-color: alpha(@accent_bg_color, 0.12);
  border-bottom: 1px solid alpha(@window_fg_color, 0.08);
  padding: 6px 12px;
}}

.omagrow-banner label {{
  font-size: 0.9em;
}}

.omagrow-banner-action {{
  font-size: 0.82em;
  padding: 2px 10px;
}}

.omagrow-calendar-month {{
  font-size: 0.9em;
  font-weight: 600;
}}

.omagrow-calendar-today {{
  padding: 2px 8px;
  font-size: 0.78em;
}}

.omagrow-calendar-day {{
  padding: 0;
  min-height: 38px;
  border-radius: 8px;
  border: 1px solid transparent;
  background-color: alpha(@window_fg_color, 0.025);
}}

.omagrow-calendar-day:hover {{
  background-color: alpha(@window_fg_color, 0.07);
}}

.omagrow-calendar-day.outside {{
  opacity: 0.35;
}}

/* Today is the quieter marker; the selected day is the one the page is showing. */
.omagrow-calendar-day.today {{
  background-color: alpha(@window_fg_color, 0.07);
}}

.omagrow-calendar-day.selected {{
  background-color: alpha(@accent_bg_color, 0.32);
  border: 1px solid alpha(@accent_bg_color, 0.75);
}}

.omagrow-calendar-number {{
  font-size: 0.78em;
  color: alpha(@window_fg_color, 0.82);
}}

.omagrow-calendar-day.today .omagrow-calendar-number {{
  color: @accent_color;
  font-weight: 700;
}}

.omagrow-calendar-day.selected .omagrow-calendar-number {{
  color: @accent_color;
  font-weight: 700;
}}

.omagrow-dot {{
  min-width: 5px;
  min-height: 5px;
  border-radius: 999px;
  background-color: alpha(@window_fg_color, 0.35);
}}

.omagrow-dot.stage {{
  background-color: @omagrow_green;
}}

.omagrow-dot.sow {{
  background-color: @warning_color;
}}

.omagrow-dot.transplant {{
  background-color: @accent_bg_color;
}}

.omagrow-dot.harvest {{
  background-color: @error_color;
}}

.omagrow-dot.flower {{
  background-color: @warning_color;
}}

.omagrow-dot.care {{
  background-color: @accent_color;
}}

.omagrow-dot.dormant {{
  background-color: alpha(@window_fg_color, 0.4);
}}

.omagrow-dot.problem {{
  background-color: @error_color;
}}

.omagrow-dot.note {{
  background-color: alpha(@window_fg_color, 0.55);
}}

.omagrow-diary-card {{
  padding: 16px;
}}

.omagrow-help-eyebrow {{
  font-size: 0.78em;
  font-weight: 700;
  color: @omagrow_green;
}}

.omagrow-help-title {{
  font-size: 1.7em;
  font-weight: 800;
}}

.omagrow-help-hero-title {{
  font-size: 2.2em;
  font-weight: 800;
}}

.omagrow-help-lead {{
  font-size: 1.1em;
  color: alpha(@window_fg_color, 0.72);
}}

.omagrow-help-body {{
  color: alpha(@window_fg_color, 0.68);
}}

.omagrow-help-shot {{
  border-radius: 14px;
  border: 1px solid alpha(@window_fg_color, 0.12);
  background-color: @card_bg_color;
}}

.omagrow-help-badge {{
  border-radius: 999px;
  padding: 6px;
  background-color: @omagrow_green;
  color: @accent_fg_color;
}}

.omagrow-help-chip {{
  border-radius: 999px;
  padding: 4px 10px;
  font-size: 0.86em;
  background-color: alpha(@omagrow_green, 0.14);
  color: alpha(@window_fg_color, 0.8);
}}

.omagrow-help-credits {{
  padding: 20px;
  border-radius: 14px;
  background-color: alpha(@window_fg_color, 0.04);
}}

.omagrow-help-credit-heading {{
  font-size: 1.15em;
  font-weight: 800;
}}

.omagrow-help-credit-title {{
  font-weight: 700;
}}

.omagrow-diary-row {{
  padding: 10px 12px;
  border-radius: 10px;
  background-color: alpha(@window_fg_color, 0.025);
}}

.omagrow-diary-dot {{
  min-width: 8px;
  min-height: 8px;
  border-radius: 999px;
  background-color: alpha(@window_fg_color, 0.35);
}}

.omagrow-diary-dot.care {{
  background-color: @accent_color;
}}

.omagrow-diary-dot.problem {{
  background-color: @error_color;
}}

.omagrow-diary-dot.note {{
  background-color: alpha(@window_fg_color, 0.55);
}}

.omagrow-diary-dot.harvest {{
  background-color: @warning_color;
}}

.omagrow-diary-dot.stage {{
  background-color: @omagrow_green;
}}

.omagrow-status-button {{
  padding: 0;
  min-height: 0;
  border-radius: 999px;
}}

.omagrow-status-button:hover {{
  background-color: alpha(@window_fg_color, 0.06);
}}

.omagrow-axis {{
  font-size: 0.75em;
  color: alpha(@window_fg_color, 0.5);
}}

.omagrow-empty-band {{
  font-style: italic;
}}

.omagrow-completeness {{
  background-color: alpha(@window_fg_color, 0.04);
  border: 1px solid {line};
  border-radius: 12px;
  padding: 16px;
}}

.omagrow-completeness.is-ready {{
  border-color: alpha(@omagrow_green, 0.55);
}}

.omagrow-completeness progressbar trough {{
  min-height: 6px;
  border-radius: 999px;
}}

.omagrow-completeness progressbar progress {{
  min-height: 6px;
  border-radius: 999px;
  background-color: @accent_bg_color;
}}

.omagrow-completeness.is-ready progressbar progress {{
  background-color: @omagrow_green;
}}

.omagrow-stage-bar {{
  min-height: 8px;
  border-radius: 999px;
  background-color: alpha(@accent_bg_color, 0.8);
}}

.omagrow-hero-button {{
  background-color: alpha(@window_bg_color, 0.72);
  color: @window_fg_color;
  border-radius: 8px;
  padding: 4px 10px;
}}

.omagrow-tile {{
  background-color: alpha(@window_fg_color, 0.05);
  border-radius: 10px;
  padding: 10px 14px;
}}

.omagrow-tile-caption {{
  font-size: 0.8em;
  color: alpha(@window_fg_color, 0.62);
}}

.omagrow-tile-value {{
  font-size: 1.05em;
  font-weight: 600;
}}
"
        )
    }
}

fn foreground_on(hex: &str) -> &'static str {
    // The equal-contrast point between black and white text is ~0.179, not the midpoint.
    if luminance(hex) > 0.179 {
        "#000000"
    } else {
        "#ffffff"
    }
}

/// Accepts `#rgb` and `#rrggbb`, and returns the six-digit lowercase form. Anything else is
/// ignored instead of being pasted into the stylesheet.
fn normalize_hex(value: &str) -> Option<String> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return None;
    }

    let expanded = match hex.len() {
        3 => hex
            .chars()
            .flat_map(|character| [character, character])
            .collect::<String>(),
        6 => hex.to_string(),
        _ => return None,
    };

    Some(format!("#{}", expanded.to_ascii_lowercase()))
}

fn rgb(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 {
        return (30, 30, 30);
    }

    let parse = |range: std::ops::Range<usize>| u8::from_str_radix(&hex[range], 16).unwrap_or(30);

    (parse(0..2), parse(2..4), parse(4..6))
}

fn mix(a: &str, b: &str, t: f32) -> String {
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let blend = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;

    format!(
        "#{:02x}{:02x}{:02x}",
        blend(ar, br),
        blend(ag, bg),
        blend(ab, bb)
    )
}

fn rgba(hex: &str, alpha: f32) -> String {
    let (r, g, b) = rgb(hex);
    format!("rgba({r}, {g}, {b}, {alpha:.3})")
}

fn luminance(hex: &str) -> f32 {
    let (r, g, b) = rgb(hex);
    let linear = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };

    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

#[cfg(test)]
mod tests {
    use super::{foreground_on, normalize_hex};

    #[test]
    fn a_light_background_gets_dark_text() {
        assert_eq!(foreground_on("#aeab94"), "#000000");
        assert_eq!(foreground_on("#1e1e1e"), "#ffffff");
    }

    #[test]
    fn only_valid_hex_reaches_the_css() {
        assert_eq!(normalize_hex("#AbC").as_deref(), Some("#aabbcc"));
        assert_eq!(normalize_hex("#112233").as_deref(), Some("#112233"));
        assert!(normalize_hex("#12345").is_none());
        assert!(normalize_hex("red").is_none());
        assert!(normalize_hex("abc").is_none());
        assert!(normalize_hex("#1e1e\" # comment").is_none());
    }
}
