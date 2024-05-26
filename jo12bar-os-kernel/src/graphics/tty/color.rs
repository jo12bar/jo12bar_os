//! Text colors for the terminal.
//!
//! The defaults use the Catppuccin Mocha colors from Alacritty's repository:
//! <https://github.com/alacritty/alacritty-theme/blob/94e1dc0b9511969a426208fbba24bd7448493785/themes/catppuccin_mocha.toml>

use embedded_graphics::pixelcolor::Rgb888;
use thiserror::Error;

/// A color as used by ANSI control sequences. This can either be an index into
/// one of the color maps or a true [`Rgb888`].
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TextColor {
    /// The default color
    Default,

    /// The default background color
    DefaultBackground,

    /// Index into the normal color map.
    ///
    /// Only indexes `0..=7` are valid.
    Normal(u8),

    /// Index into the bright color map.
    ///
    /// Only indexes `0..=7` are valid.
    Bright(u8),

    /// Index into the extended color map.
    ///
    /// Any `u8` value is a valid index here.
    Extended(u8),

    /// A true RGB color for using a `u8` per-channel.
    True(Rgb888),
}

impl TryInto<Rgb888> for TextColor {
    type Error = TextColorError;

    fn try_into(self) -> Result<Rgb888, Self::Error> {
        match self {
            TextColor::Default => Ok(DEFAULT_TEXT),
            TextColor::DefaultBackground => Ok(DEFAULT_BACKGROUND),

            TextColor::Normal(i) if i <= 7 => Ok(NORMAL_COLORS[i as usize]),
            TextColor::Normal(i) => Err(TextColorError::IndexOutOfBounds {
                index: i,
                max_index: 7,
            }),

            TextColor::Bright(i) if i <= 7 => Ok(BRIGHT_COLORS[i as usize]),
            TextColor::Bright(i) => Err(TextColorError::IndexOutOfBounds {
                index: i,
                max_index: 7,
            }),

            // TODO: Implement conversion from extended index colors to Rgb888
            TextColor::Extended(_) => Err(TextColorError::NotSupported(self)),

            TextColor::True(color) => Ok(color),
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq, Clone, Copy)]
#[allow(missing_docs)]
pub enum TextColorError {
    #[error("Text color {0:?} not supported")]
    NotSupported(TextColor),
    #[error("Color index out of bounds. Index: {index}, Max {max_index}")]
    IndexOutOfBounds { index: u8, max_index: u8 },
}

/// Default text color
pub const DEFAULT_TEXT: Rgb888 = Rgb888::new(0xcd, 0xd6, 0xf4);
/// Default text color
pub const DEFAULT_BACKGROUND: Rgb888 = Rgb888::new(0x1e, 0x1e, 0x2e);

/// Normal colors
///
/// In order, the colors are Black, Red, Green, Yellow, Blue, Magenta, Cyan, White.
pub const NORMAL_COLORS: [Rgb888; 8] = [
    Rgb888::new(0x45, 0x47, 0x5a), // Black (surface1)
    Rgb888::new(0xf3, 0x8b, 0xa8), // Red (red)
    Rgb888::new(0xa6, 0xe3, 0xa1), // Green (green)
    Rgb888::new(0xf9, 0xe2, 0xaf), // Yellow (yellow)
    Rgb888::new(0x89, 0xb4, 0xfa), // Blue (blue)
    Rgb888::new(0xf5, 0xc2, 0xe7), // Magenta (pink)
    Rgb888::new(0x94, 0xe2, 0xd5), // Cyan (teal)
    Rgb888::new(0xba, 0xc2, 0xde), // White (subtext1)
];

/// Bright colors
///
/// In order, the colors are Black, Red, Green, Yellow, Blue, Magenta, Cyan, White.
pub const BRIGHT_COLORS: [Rgb888; 8] = [
    Rgb888::new(0x58, 0x5b, 0x70), // Black (surface2)
    Rgb888::new(0xf3, 0x8b, 0xa8), // Red (red)
    Rgb888::new(0xa6, 0xe3, 0xa1), // Green (green)
    Rgb888::new(0xf9, 0xe2, 0xaf), // Yellow (yellow)
    Rgb888::new(0x89, 0xb4, 0xfa), // Blue (blue)
    Rgb888::new(0xf5, 0xc2, 0xe7), // Magenta (pink)
    Rgb888::new(0x94, 0xe2, 0xd5), // Cyan (teal)
    Rgb888::new(0xa6, 0xad, 0xc8), // White (subtext0)
];
