//! Colours and gradients. A [`Theme`] turns semantic roles ("muted",
//! "usage at 70 %", "temperature near TjMax") into styles, and degrades to
//! plain attributes when `NO_COLOR` is set.

use ratatui::style::{Color, Modifier, Style};

use crate::temp::Reading;
use crate::topology::CoreKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// A gradient: `(position in 0..=1, colour)`, positions ascending.
pub type Stops = [(f32, Rgb)];

// A calm, low-saturation palette (Nord-like) that reads well on dark and
// light backgrounds.
pub const BORDER: Rgb = Rgb(0x4c, 0x56, 0x6a);
pub const ACCENT: Rgb = Rgb(0x88, 0xc0, 0xd0);
pub const MUTED: Rgb = Rgb(0x7b, 0x84, 0x96);
pub const TRACK: Rgb = Rgb(0x43, 0x4c, 0x5e);
pub const CACHE: Rgb = Rgb(0x5e, 0x69, 0x82);
pub const P_CORE: Rgb = Rgb(0xb4, 0x8e, 0xad);
pub const E_CORE: Rgb = Rgb(0x8f, 0xbc, 0xbb);

/// Usage: green → yellow → orange → red.
pub const USAGE: &Stops = &[
    (0.0, Rgb(0xa3, 0xbe, 0x8c)),
    (0.5, Rgb(0xeb, 0xcb, 0x8b)),
    (0.8, Rgb(0xd0, 0x87, 0x70)),
    (1.0, Rgb(0xbf, 0x61, 0x6a)),
];

/// Temperature, on the chip-relative scale of [`crate::temp::Limits::heat`]:
/// cool blue → teal → warm amber → hot red.
pub const HEAT: &Stops = &[
    (0.0, Rgb(0x81, 0xa1, 0xc1)),
    (0.3, Rgb(0x8f, 0xbc, 0xbb)),
    (0.6, Rgb(0xeb, 0xcb, 0x8b)),
    (0.85, Rgb(0xd0, 0x87, 0x70)),
    (1.0, Rgb(0xbf, 0x61, 0x6a)),
];

/// Linear interpolation between gradient stops; `t` is clamped.
pub fn gradient(stops: &Stops, t: f32) -> Rgb {
    let t = if t.is_finite() {
        t.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let Some(&(_, first)) = stops.first() else {
        return Rgb(0, 0, 0);
    };
    let mut prev = (0.0, first);
    for &(pos, color) in stops {
        if t <= pos {
            let span = pos - prev.0;
            let f = if span > 0.0 { (t - prev.0) / span } else { 1.0 };
            return lerp(prev.1, color, f);
        }
        prev = (pos, color);
    }
    prev.1
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// Nearest colour in the xterm 256-colour palette (6×6×6 cube or grey ramp).
pub fn ansi256(Rgb(r, g, b): Rgb) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |c: u8| match c {
        0..48 => 0,
        48..115 => 1,
        _ => (c - 35) / 40,
    };
    let dist = |(r2, g2, b2): (u8, u8, u8)| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(r, r2) + d(g, g2) + d(b, b2)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (
        LEVELS[ri as usize],
        LEVELS[gi as usize],
        LEVELS[bi as usize],
    );
    let avg = ((r as u16 + g as u16 + b as u16) / 3) as u8;
    let gray_i = if avg > 238 {
        23
    } else {
        avg.saturating_sub(3) / 10
    };
    let gray = 8 + 10 * gray_i;
    if dist((gray, gray, gray)) < dist(cube) {
        232 + gray_i
    } else {
        16 + 36 * ri + 6 * gi + bi
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    /// `NO_COLOR`: bars and text only.
    Mono,
    Ansi256,
    TrueColor,
}

impl ColorMode {
    /// `NO_COLOR` set to any non-empty value wins (see no-color.org);
    /// `COLORTERM=truecolor|24bit` enables RGB, otherwise 256 colours.
    pub fn detect(no_color: Option<&str>, colorterm: Option<&str>) -> Self {
        if no_color.is_some_and(|v| !v.is_empty()) {
            ColorMode::Mono
        } else if matches!(colorterm, Some("truecolor" | "24bit")) {
            ColorMode::TrueColor
        } else {
            ColorMode::Ansi256
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub mode: ColorMode,
}

impl Theme {
    pub fn new(mode: ColorMode) -> Self {
        Theme { mode }
    }

    pub fn mono(&self) -> bool {
        self.mode == ColorMode::Mono
    }

    pub fn color(&self, c: Rgb) -> Option<Color> {
        match self.mode {
            ColorMode::Mono => None,
            ColorMode::Ansi256 => Some(Color::Indexed(ansi256(c))),
            ColorMode::TrueColor => Some(Color::Rgb(c.0, c.1, c.2)),
        }
    }

    pub fn fg(&self, c: Rgb) -> Style {
        self.color(c)
            .map_or_else(Style::new, |c| Style::new().fg(c))
    }

    /// Secondary text. Mono mode keeps plain text for legibility.
    pub fn muted(&self) -> Style {
        self.fg(MUTED)
    }

    pub fn border(&self) -> Style {
        self.fg(BORDER)
    }

    pub fn title(&self) -> Style {
        self.fg(ACCENT).add_modifier(Modifier::BOLD)
    }

    pub fn accent(&self) -> Style {
        self.fg(ACCENT)
    }

    pub fn bold(&self) -> Style {
        Style::new().add_modifier(Modifier::BOLD)
    }

    pub fn dim(&self) -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }

    pub fn track(&self) -> Style {
        if self.mono() {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            self.fg(TRACK)
        }
    }

    pub fn usage(&self, frac: f32) -> Style {
        self.fg(gradient(USAGE, frac))
    }

    pub fn heat(&self, r: &Reading) -> Style {
        let style = self.fg(gradient(HEAT, r.heat()));
        if r.critical() {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    }

    pub fn kind(&self, kind: CoreKind) -> Style {
        match kind {
            CoreKind::Performance => self.fg(P_CORE),
            CoreKind::Efficiency => self.fg(E_CORE),
            CoreKind::Uniform => self.accent(),
        }
        .add_modifier(Modifier::BOLD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_endpoints_and_midpoints() {
        let stops: &Stops = &[(0.0, Rgb(0, 0, 0)), (1.0, Rgb(200, 100, 50))];
        assert_eq!(gradient(stops, 0.0), Rgb(0, 0, 0));
        assert_eq!(gradient(stops, 1.0), Rgb(200, 100, 50));
        assert_eq!(gradient(stops, 0.5), Rgb(100, 50, 25));
        assert_eq!(gradient(stops, -3.0), Rgb(0, 0, 0));
        assert_eq!(gradient(stops, 9.0), Rgb(200, 100, 50));
        assert_eq!(gradient(stops, f32::NAN), Rgb(0, 0, 0));
        assert_eq!(gradient(USAGE, 0.5), USAGE[1].1);
    }

    #[test]
    fn ansi256_mapping() {
        assert_eq!(ansi256(Rgb(0, 0, 0)), 16);
        assert_eq!(ansi256(Rgb(255, 255, 255)), 231);
        assert_eq!(ansi256(Rgb(255, 0, 0)), 196);
        assert_eq!(ansi256(Rgb(128, 128, 128)), 244);
        assert_eq!(ansi256(Rgb(0x5f, 0x87, 0xaf)), 67);
    }

    #[test]
    fn color_mode_detection() {
        assert_eq!(
            ColorMode::detect(Some("1"), Some("truecolor")),
            ColorMode::Mono
        );
        // An empty NO_COLOR does not disable colour.
        assert_eq!(
            ColorMode::detect(Some(""), Some("truecolor")),
            ColorMode::TrueColor
        );
        assert_eq!(ColorMode::detect(None, Some("24bit")), ColorMode::TrueColor);
        assert_eq!(ColorMode::detect(None, None), ColorMode::Ansi256);
    }

    #[test]
    fn mono_theme_has_no_colors() {
        let t = Theme::new(ColorMode::Mono);
        assert_eq!(t.usage(0.9).fg, None);
        assert_eq!(t.border().fg, None);
        assert!(t.track().add_modifier.contains(Modifier::DIM));
    }
}
