//! The one colour palette of the manager: No Man's Sky interface blue, amber for attention, green for Play, red only for danger.
//! Every colour the window draws comes from here (a unit test greps the sources for strays).
//! `dev/make_app_icon.py` uses the same blue and amber values for the icon.

use egui::Color32;

// Primary blue (the NMS interface blue, about #2D7DD2).
pub const PRIMARY: Color32 = Color32::from_rgb(45, 125, 210);
pub const PRIMARY_LIGHT: Color32 = Color32::from_rgb(120, 180, 245);
pub const PRIMARY_DARK: Color32 = Color32::from_rgb(26, 78, 142);

// Red means removal, bad or an error, and nothing else: Remove, Remove all, clean-up, failures.
pub const ACCENT_RED: Color32 = Color32::from_rgb(226, 84, 84);
pub const ACCENT_RED_LIGHT: Color32 = Color32::from_rgb(255, 140, 130);

// Amber: something needs a look but is not bad (an update is waiting, a warning).
pub const ATTENTION: Color32 = Color32::from_rgb(238, 170, 40);
pub const ATTENTION_LIGHT: Color32 = Color32::from_rgb(255, 208, 110);
pub const TEXT_ON_ATTENTION: Color32 = Color32::from_rgb(34, 24, 4);

// Play: the one green call to action in the header.
pub const PLAY: Color32 = Color32::from_rgb(30, 160, 108);
pub const PLAY_LIGHT: Color32 = Color32::from_rgb(128, 228, 182);
pub const TEXT_ON_PLAY: Color32 = Color32::WHITE;

// Surfaces, darkest to lightest.
pub const BG: Color32 = Color32::from_rgb(14, 20, 31);
pub const SURFACE_SUNKEN: Color32 = Color32::from_rgb(10, 15, 24);
pub const SURFACE_STRIP: Color32 = Color32::from_rgb(18, 26, 40);
pub const SURFACE_CARD: Color32 = Color32::from_rgb(26, 37, 56);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(38, 52, 76);
pub const EDGE: Color32 = Color32::from_rgb(50, 68, 98);
pub const TRACK_OFF: Color32 = Color32::from_rgb(58, 70, 92);

// Elevation (see `style::elevation`): 0 window, 1 panels (chrome, sidebar, footer), 2 cards and
// setting panels, 3 a hovered card (one level up).
pub const SURFACE_0: Color32 = BG;
pub const SURFACE_1: Color32 = Color32::from_rgb(19, 27, 41);
pub const SURFACE_2: Color32 = SURFACE_CARD;
pub const SURFACE_3: Color32 = Color32::from_rgb(33, 47, 70);

// Header band gradient.
pub const HEADER_TOP: Color32 = Color32::from_rgb(36, 102, 178);
pub const HEADER_BOTTOM: Color32 = Color32::from_rgb(13, 36, 74);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(228, 234, 244);
pub const TEXT_STRONG: Color32 = Color32::WHITE;
pub const TEXT_MUTED: Color32 = Color32::from_rgb(145, 158, 180);
pub const SWITCH_KNOB: Color32 = Color32::from_rgb(244, 247, 252);
pub const SHADOW: Color32 = Color32::from_black_alpha(90);

// Status. Live is a cool green-cyan; warnings are amber; only errors are red.
pub const SUCCESS: Color32 = Color32::from_rgb(70, 196, 140);
pub const WARN: Color32 = ATTENTION_LIGHT;
pub const DANGER: Color32 = ACCENT_RED;
pub const INFO: Color32 = PRIMARY_LIGHT;
pub const PARKED: Color32 = Color32::from_rgb(120, 134, 158);

// Notice fill (Linux note).
pub const NOTICE_FILL: Color32 = Color32::from_rgb(42, 35, 22);

// Group colours (badge and heading): blue, green, cyan, violet and slate; never red or amber.
pub const GROUP_INVENTORY: Color32 = Color32::from_rgb(45, 125, 210);
pub const GROUP_CRAFTING: Color32 = Color32::from_rgb(92, 168, 62);
pub const GROUP_MAP: Color32 = Color32::from_rgb(64, 208, 224);
pub const GROUP_INTERFACE: Color32 = Color32::from_rgb(138, 102, 232);
pub const GROUP_OTHER: Color32 = Color32::from_rgb(100, 116, 142);

// Button feedback: hover mixes toward the light, press toward the dark.
pub const HOVER_TOWARD: Color32 = Color32::WHITE;
pub const PRESS_TOWARD: Color32 = Color32::BLACK;

/// `a` moved toward `b` by `t` (0..=1), per channel in sRGB with alpha.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let (a, b) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t.clamp(0.0, 1.0)).round() as u8;
    Color32::from_rgba_unmultiplied(ch(0), ch(1), ch(2), ch(3))
}

#[cfg(test)]
mod tests {
    /// No colour literal outside this file: the UI sources must take every colour from `palette`.
    #[test]
    fn no_stray_colour_literals() {
        let needles = [["Color32::", "from_rgb"].concat(), ["Color32::", "from_rgba"].concat(), ["Color32::", "from_gray"].concat()];
        for (name, src) in [("ui.rs", include_str!("ui.rs")), ("cards.rs", include_str!("cards.rs")), ("style.rs", include_str!("style.rs"))] {
            for needle in &needles {
                assert!(!src.contains(needle.as_str()), "{name} has a colour literal ({needle}); use palette.rs");
            }
        }
    }

    fn reddish(c: egui::Color32) -> bool {
        let (r, g, b) = (c.r() as i32, c.g() as i32, c.b() as i32);
        r > g + 50 && r > b + 50
    }

    /// Red is only for removal, errors and bad news: nothing else may be a red colour.
    #[test]
    fn only_danger_colours_are_red() {
        use super::*;
        assert!(reddish(ACCENT_RED) && reddish(DANGER) && reddish(ACCENT_RED_LIGHT));
        for (name, c) in [
            ("PLAY", PLAY),
            ("PLAY_LIGHT", PLAY_LIGHT),
            ("ATTENTION", ATTENTION),
            ("ATTENTION_LIGHT", ATTENTION_LIGHT),
            ("WARN", WARN),
            ("SUCCESS", SUCCESS),
            ("GROUP_INVENTORY", GROUP_INVENTORY),
            ("GROUP_CRAFTING", GROUP_CRAFTING),
            ("GROUP_MAP", GROUP_MAP),
            ("GROUP_INTERFACE", GROUP_INTERFACE),
            ("GROUP_OTHER", GROUP_OTHER),
            ("PRIMARY", PRIMARY),
            ("PRIMARY_LIGHT", PRIMARY_LIGHT),
        ] {
            // Amber and gold have a high red channel too, but also a high green one.
            let amber = c.g() as i32 > c.b() as i32 + 60 && c.r() > 200;
            assert!(!reddish(c) || amber, "{name} looks red");
        }
        // The group colours are told apart by hue: no two within a small distance.
        let groups = [GROUP_INVENTORY, GROUP_CRAFTING, GROUP_MAP, GROUP_INTERFACE, GROUP_OTHER];
        for (i, a) in groups.iter().enumerate() {
            for b in &groups[i + 1..] {
                let d = (a.r() as i32 - b.r() as i32).abs() + (a.g() as i32 - b.g() as i32).abs() + (a.b() as i32 - b.b() as i32).abs();
                assert!(d > 90, "group colours too close: {a:?} {b:?}");
            }
        }
    }
}
