//! Colour parsing and the conversions each target needs.
//!
//! Every theme stores colours as `#rrggbb`. The targets disagree about what
//! they want back: iTerm2 wants floats between 0 and 1, AppleScript wants
//! 16-bit integers, and powerlevel10k only understands the 256-colour palette.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A 24-bit colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("`{0}` is not a colour like #1e66f5")]
pub struct ParseColorError(String);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#rrggbb`, `rrggbb`, `#rgb` or `rgb`.
    pub fn parse(text: &str) -> Result<Self, ParseColorError> {
        let body = text.trim().trim_start_matches('#');
        let fail = || ParseColorError(text.to_string());

        let expand = |c: u8| -> u8 {
            // "f" in a three-digit hex means "ff", not "f0".
            c * 17
        };
        let nibble = |byte: u8| -> Option<u8> { (byte as char).to_digit(16).map(|d| d as u8) };

        match body.len() {
            3 => {
                let bytes = body.as_bytes();
                let r = nibble(bytes[0]).ok_or_else(fail)?;
                let g = nibble(bytes[1]).ok_or_else(fail)?;
                let b = nibble(bytes[2]).ok_or_else(fail)?;
                Ok(Self::new(expand(r), expand(g), expand(b)))
            }
            6 => {
                let value = u32::from_str_radix(body, 16).map_err(|_| fail())?;
                Ok(Self::new(
                    ((value >> 16) & 0xff) as u8,
                    ((value >> 8) & 0xff) as u8,
                    (value & 0xff) as u8,
                ))
            }
            _ => Err(fail()),
        }
    }

    /// `#rrggbb`, which is what every config file we write wants.
    pub fn hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// The same colour without the leading `#`, for fzf and iTerm2 preset files.
    pub fn bare_hex(&self) -> String {
        format!("{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// iTerm2 stores each channel as a float between 0 and 1 in its plist.
    pub fn components(&self) -> (f64, f64, f64) {
        (
            self.r as f64 / 255.0,
            self.g as f64 / 255.0,
            self.b as f64 / 255.0,
        )
    }

    /// AppleScript colour properties take three 16-bit integers.
    pub fn applescript(&self) -> String {
        let scale = |c: u8| (c as u32 * 65535) / 255;
        format!(
            "{{{}, {}, {}}}",
            scale(self.r),
            scale(self.g),
            scale(self.b)
        )
    }

    /// Relative luminance per WCAG, used to decide whether a theme reads as
    /// light or dark and to pick readable text over a swatch.
    pub fn luminance(&self) -> f64 {
        let channel = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.r) + 0.7152 * channel(self.g) + 0.0722 * channel(self.b)
    }

    /// The nearest colour in the xterm 256-colour palette.
    ///
    /// powerlevel10k takes palette indices rather than truecolor hex, so every
    /// prompt colour has to survive this trip. Indices 0-15 are excluded on
    /// purpose: those are the terminal's own ANSI slots, which we are also
    /// rewriting, so matching against them would make the prompt colour depend
    /// on the very palette it sits on.
    pub fn to_xterm256(&self) -> u8 {
        let mut best_index = 16u8;
        let mut best_distance = u32::MAX;

        for index in 16..=255u8 {
            let candidate = xterm256_color(index);
            let distance = self.distance_squared(&candidate);
            if distance < best_distance {
                best_distance = distance;
                best_index = index;
            }
        }

        best_index
    }

    fn distance_squared(&self, other: &Rgb) -> u32 {
        // Weighted so the comparison tracks perceived difference rather than
        // raw channel distance; green dominates how bright a colour looks.
        let dr = (self.r as i32 - other.r as i32).pow(2) as u32;
        let dg = (self.g as i32 - other.g as i32).pow(2) as u32;
        let db = (self.b as i32 - other.b as i32).pow(2) as u32;
        2 * dr + 4 * dg + 3 * db
    }
}

/// The RGB value of one slot in the xterm 256-colour palette, for slots 16-255.
///
/// 16-231 is a 6x6x6 colour cube; 232-255 is a 24-step greyscale ramp.
fn xterm256_color(index: u8) -> Rgb {
    if index < 16 {
        // Not used by `to_xterm256`; the standard ANSI slots vary by terminal,
        // so there is no correct answer here. Black keeps the function total.
        return Rgb::new(0, 0, 0);
    }

    if index < 232 {
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        let offset = index as u16 - 16;
        let r = LEVELS[(offset / 36) as usize];
        let g = LEVELS[((offset % 36) / 6) as usize];
        let b = LEVELS[(offset % 6) as usize];
        Rgb::new(r, g, b)
    } else {
        let level = 8 + (index as u16 - 232) * 10;
        let level = level as u8;
        Rgb::new(level, level, level)
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.hex())
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Rgb::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_forms_a_theme_file_might_use() {
        assert_eq!(Rgb::parse("#1e66f5").unwrap(), Rgb::new(0x1e, 0x66, 0xf5));
        assert_eq!(Rgb::parse("1e66f5").unwrap(), Rgb::new(0x1e, 0x66, 0xf5));
        assert_eq!(Rgb::parse("  #EFF1F5 ").unwrap(), Rgb::new(0xef, 0xf1, 0xf5));
        assert_eq!(Rgb::parse("#fff").unwrap(), Rgb::new(255, 255, 255));
        assert_eq!(Rgb::parse("#f0a").unwrap(), Rgb::new(255, 0, 170));
    }

    #[test]
    fn rejects_things_that_are_not_colours() {
        for bad in ["", "#", "#12", "#12345", "#gggggg", "blue", "#1234567"] {
            assert!(Rgb::parse(bad).is_err(), "{bad} should not parse");
        }
    }

    #[test]
    fn hex_round_trips() {
        let color = Rgb::new(0x4c, 0x4f, 0x69);
        assert_eq!(color.hex(), "#4c4f69");
        assert_eq!(color.bare_hex(), "4c4f69");
        assert_eq!(Rgb::parse(&color.hex()).unwrap(), color);
    }

    #[test]
    fn iterm_components_span_zero_to_one() {
        let (r, g, b) = Rgb::new(255, 0, 128).components();
        assert_eq!(r, 1.0);
        assert_eq!(g, 0.0);
        assert!((b - 0.50196).abs() < 1e-4);
    }

    #[test]
    fn applescript_scales_to_sixteen_bit() {
        assert_eq!(Rgb::new(255, 0, 0).applescript(), "{65535, 0, 0}");
        assert_eq!(Rgb::new(0, 0, 0).applescript(), "{0, 0, 0}");
    }

    #[test]
    fn luminance_separates_light_from_dark_backgrounds() {
        let latte = Rgb::parse("#eff1f5").unwrap();
        let mocha = Rgb::parse("#1e1e2e").unwrap();
        assert!(latte.luminance() > 0.5, "latte should read as light");
        assert!(mocha.luminance() < 0.1, "mocha should read as dark");
    }

    #[test]
    fn xterm256_lands_on_exact_palette_entries() {
        // 231 is pure white in the colour cube, 16 is pure black.
        assert_eq!(Rgb::new(255, 255, 255).to_xterm256(), 231);
        assert_eq!(Rgb::new(0, 0, 0).to_xterm256(), 16);
    }

    #[test]
    fn xterm256_never_returns_an_ansi_slot() {
        // Slots 0-15 are the ones we rewrite, so a prompt colour must not
        // resolve to one or it would chase its own palette.
        for hex in ["#eff1f5", "#1e1e2e", "#d20f39", "#40a02b", "#8839ef"] {
            let index = Rgb::parse(hex).unwrap().to_xterm256();
            assert!(index >= 16, "{hex} resolved to reserved slot {index}");
        }
    }

    #[test]
    fn xterm256_stays_close_to_the_requested_colour() {
        let wanted = Rgb::parse("#1e66f5").unwrap();
        let got = xterm256_color(wanted.to_xterm256());
        let per_channel = |a: u8, b: u8| (a as i32 - b as i32).abs();
        assert!(per_channel(wanted.r, got.r) < 40);
        assert!(per_channel(wanted.g, got.g) < 40);
        assert!(per_channel(wanted.b, got.b) < 40);
    }

    #[test]
    fn greyscale_ramp_is_reachable() {
        // #808080 sits in the 232-255 ramp, not the colour cube.
        let index = Rgb::new(0x80, 0x80, 0x80).to_xterm256();
        let got = xterm256_color(index);
        assert!(got.r == got.g && got.g == got.b, "expected a grey, got {got}");
    }
}
