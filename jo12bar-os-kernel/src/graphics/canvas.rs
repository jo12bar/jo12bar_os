//! A [Canvas] represents a surface that can be drawn to.
//!
//! This can be a framebuffer, and image, or anything else.

use core::fmt::Write;
use derive_builder::Builder;
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{MonoFont, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::{Alignment, Baseline, LineHeight, Text, TextStyleBuilder},
};
use thiserror::Error;

use super::tty;

/// Something that you can draw graphics and text to, and potentially scroll vertically.
pub trait Canvas: DrawTarget {
    /// The width in pixels.
    fn width(&self) -> u32 {
        self.bounding_box().size.width
    }

    /// The height in pixels.
    fn height(&self) -> u32 {
        self.bounding_box().size.height
    }

    /// Returns true if this implementation of Canvas supports scrolling.
    fn supports_scrolling() -> bool;

    /// Set the pixel at `(x, y)` to `color`
    fn set_pixel(&mut self, x: u32, y: u32, c: <Self as DrawTarget>::Color);

    /// Scrolls the canvas by `height` pixels
    ///
    /// A positive `height` means that every row of pixels is moved up by
    /// `height` pixels and the bottom rows of pixels are cleard to `clear_color`.
    fn scroll(
        &mut self,
        height: i32,
        clear_color: <Self as DrawTarget>::Color,
    ) -> Result<(), ScrollingNotSupportedError>;
}

/// Scrolling is not supported
///
/// returned by scrolling actions in a canvas if scrolling is not supported
/// by the canvas
#[derive(Debug, Error, PartialEq, Eq, Clone, Copy)]
pub struct ScrollingNotSupportedError;

impl core::fmt::Display for ScrollingNotSupportedError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Scrolling is not supported")
    }
}

/// Enum for all canvas writer related errors
#[derive(Error, Debug, PartialEq, Eq, Clone, Copy)]
#[allow(missing_docs)]
pub enum CanvasWriterError {
    #[error("scrolling is not supported for this canvas writer")]
    ScrollingNotSupported(ScrollingNotSupportedError),
    #[error("failed to parse ansi control sequence: {0}")]
    SGRParsing(tty::SGRParseError),
    #[error("feature {0} is not implemented")]
    Todo(&'static str),
    #[error("Color pallet not supported for {0}")]
    ColorError(tty::TextColorError),
}

impl From<tty::SGRParseError> for CanvasWriterError {
    fn from(value: tty::SGRParseError) -> Self {
        CanvasWriterError::SGRParsing(value)
    }
}

impl From<tty::TextColorError> for CanvasWriterError {
    fn from(value: tty::TextColorError) -> Self {
        CanvasWriterError::ColorError(value)
    }
}
/// A [`Write`]r for a [`Canvas`]
#[derive(Debug, Builder)]
#[builder(
    no_std,
    pattern = "owned",
    build_fn(validate = "Self::validate", error = "CanvasWriterBuilderError")
)]
//#[doc = "A Builder for a [CanvasWriter]"]
pub struct CanvasWriter<'font, C>
where
    C: Canvas + DrawTarget<Color = Rgb888>,
{
    /// The [Canvas] to write to
    canvas: C,

    /// The [MonoFont] used for the text.
    font: MonoFont<'font>,

    /// how much to indent the next line.
    #[builder(default = "0")]
    pub indent_line: i32,

    /// Left magin between the text and the canvas edge in pixels.
    #[builder(default = "0")]
    margin_left: i32,
    /// Left magin between the text and the canvas edge in pixels.
    #[builder(default = "0")]
    margin_right: i32,
    /// Left magin between the text and the canvas edge in pixels.
    #[builder(default = "0")]
    margin_top: i32,
    /// Left magin between the text and the canvas edge in pixels.
    #[builder(default = "0")]
    margin_bottom: i32,

    /// Initial position of the cursor.
    #[builder(default = "self._build_cursor()", setter(skip))]
    cursor: Point,

    /// The active text color.
    #[builder(
        default = "self.default_text_color.unwrap_or(tty::color::DEFAULT_TEXT)",
        setter(skip)
    )]
    text_color: <C as DrawTarget>::Color,

    /// The default text color.
    #[builder(default = "tty::color::DEFAULT_TEXT", setter(name = "text_color"))]
    default_text_color: <C as DrawTarget>::Color,

    /// Log line height.
    #[builder(default)]
    line_height: LineHeight,

    /// The active background color.
    #[builder(
        default = "self.default_text_color.unwrap_or(tty::color::DEFAULT_BACKGROUND)",
        setter(skip)
    )]
    background_color: <C as DrawTarget>::Color,

    /// The default text color.
    #[builder(
        default = "tty::color::DEFAULT_BACKGROUND",
        setter(name = "background_color")
    )]
    default_background_color: <C as DrawTarget>::Color,

    /// The default scroll behaviour of the writer.
    #[builder(default)]
    scroll_behaviour: CanvasWriterScrollBehaviour,

    /// Logs errors if set to `true`.
    ///
    /// If set to `false` if `write_str` fails with [core::fmt::Error] there
    /// is no way to get the reason of the failure.
    ///
    /// This is useful if the [CanvasWriter] is used as the target for a logger.
    #[builder(default = "true")]
    log_errors: bool,

    /// If set the writer will ignore all ansi control sequences.
    #[builder(default = "false")]
    #[cfg_attr(feature = "no-colored-log", allow(dead_code))]
    ignore_ansi: bool,
}

/// Error used by [`CanvasWriterBuilder`].
#[allow(missing_docs)]
#[derive(Debug, Error)]
pub enum CanvasWriterBuilderError {
    UninitializedField(&'static str),
    ScrollingNotSupported,
}

impl From<derive_builder::UninitializedFieldError> for CanvasWriterBuilderError {
    fn from(value: derive_builder::UninitializedFieldError) -> Self {
        Self::UninitializedField(value.field_name())
    }
}

impl core::fmt::Display for CanvasWriterBuilderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CanvasWriterBuilderError::UninitializedField(field) => write!(
                f,
                "Field \"{field}\" not initialized in CanvasWriterBuilder"
            ),
            CanvasWriterBuilderError::ScrollingNotSupported => write!(
                f,
                "Scolling not supported by Canvas used in CanvasWriterBuilder"
            ),
        }
    }
}

/// Scroll behaviour for a [`CanvasWriter`] (specifically, one that _supports_ scrolling).
#[derive(Debug, Clone, Copy, Default)]
pub enum CanvasWriterScrollBehaviour {
    /// The canvas scrolls when the end of the visible area is reached.
    ///
    /// When a new line is written and the canvas is "full" the canvas will scroll
    /// down by 1 line.
    #[default]
    Scroll,
    /// The canvas is cleared when the end of the visible area is reached.
    ///
    /// When a new line is writen and the canvas is "full" the canvas will be
    /// cleared, and the new line will be written at the top.
    #[allow(dead_code)]
    Clear,
}

impl<C> CanvasWriterBuilder<'_, C>
where
    C: Canvas + DrawTarget<Color = Rgb888>,
{
    fn _build_cursor(&self) -> Point {
        let l_margin = self.margin_left.unwrap_or(0);
        let t_margin = self.margin_top.unwrap_or(0);
        let indent = self.indent_line.unwrap_or(0);

        Point {
            x: l_margin + indent,
            y: t_margin,
        }
    }

    fn validate(&self) -> Result<(), CanvasWriterBuilderError> {
        match self.scroll_behaviour.unwrap_or_default() {
            CanvasWriterScrollBehaviour::Scroll => {
                if C::supports_scrolling() {
                    Ok(())
                } else {
                    Err(CanvasWriterBuilderError::ScrollingNotSupported)
                }
            }

            CanvasWriterScrollBehaviour::Clear => Ok(()),
        }
    }
}

impl<C> CanvasWriter<'_, C>
where
    C: Canvas + DrawTarget<Color = Rgb888>,
{
    /// Creates a [CanvasWriterBuilder]
    pub fn builder<'font>() -> CanvasWriterBuilder<'font, C> {
        CanvasWriterBuilder::create_empty()
    }

    /// Return the internally-used [Canvas].
    pub fn into_canvas(self) -> C {
        self.canvas
    }

    /// Return the absolute line height in pixels.
    #[inline]
    pub const fn absolute_line_height(&self) -> u32 {
        self.line_height
            .to_absolute(self.font.character_size.height)
    }

    /// Jump to the next line
    pub fn new_line(&mut self) {
        self.carriage_return();

        if self.cursor.y + self.absolute_line_height() as i32
            > self.canvas.height() as i32 - self.margin_bottom
        {
            match self.scroll_behaviour {
                CanvasWriterScrollBehaviour::Scroll => {
                    self.canvas
                        .scroll(self.absolute_line_height() as i32, self.background_color)
                        .expect("The builder was supposed to check that scrolling is supported, but didn't somehow");
                }
                CanvasWriterScrollBehaviour::Clear => {
                    let _ = self.canvas.clear(self.background_color);
                    self.cursor.y = self.margin_top;
                }
            }
        } else {
            self.cursor.y += self.absolute_line_height() as i32;
        }
    }

    /// Jump back to the start of the current line.
    #[inline]
    pub fn carriage_return(&mut self) {
        self.cursor.x = self.margin_left + self.indent_line;
    }

    /// Advance the cursor by 1 character.
    ///
    /// This is done automatically when calling [`print_char()`].
    #[inline]
    pub fn advance_cursor(&mut self) {
        self.cursor.x += (self.font.character_size.width + self.font.character_spacing) as i32;
        if self.cursor.x >= self.canvas.width() as i32 - self.margin_right {
            self.new_line();
        }
    }

    #[cfg(feature = "no-colored-log")]
    fn handle_ansi_ctrl_seq(
        &mut self,
        chars: &mut impl Iterator<Item = char>,
    ) -> Result<(), CanvasWriterError> {
        // Skip ANSI SGR sequence and ignore possible errors.

        use super::tty::AnsiSGR;
        let _ = AnsiSGR::parse_from_chars(chars, true);
        Ok(())
    }

    #[cfg(not(feature = "no-colored-log"))]
    /// Handles ansi colors
    ///
    /// `chars` should be the rest of the ansi control sequence fater the `ESC(0x1b)`.
    ///
    /// A sequence looks like `ESC[(0-9){1,3}(;(0-9){1,3})*m`
    /// https://chrisyeh96.github.io/2020/03/28/terminal-colors.html
    fn handle_ansi_ctrl_seq(
        &mut self,
        chars: &mut impl Iterator<Item = char>,
    ) -> Result<(), CanvasWriterError> {
        use super::tty::AnsiSGR;

        let sgr =
            AnsiSGR::parse_from_chars(chars, true).map_err(Into::<CanvasWriterError>::into)?;
        if self.ignore_ansi {
            return Ok(());
        }
        match sgr {
            AnsiSGR::Reset => self.reset_to_defaults(),
            AnsiSGR::Bold => return Err(CanvasWriterError::Todo("bold text")),
            AnsiSGR::Faint => return Err(CanvasWriterError::Todo("faint text")),
            AnsiSGR::Underline => return Err(CanvasWriterError::Todo("underlined text")),
            AnsiSGR::SlowBlink => return Err(CanvasWriterError::Todo("slow blink text")),
            AnsiSGR::Foreground(c) => self.text_color = c.try_into()?,
            AnsiSGR::Background(c) => self.background_color = c.try_into()?,
        }

        Ok(())
    }

    /// Resets the [`CanvasWriter`] to its default style values.
    ///
    /// This affects all style values, currently including:
    /// - [`text_color`]
    /// - [`background_color`]
    pub fn reset_to_defaults(&mut self) {
        self.text_color = self.default_text_color;
        self.background_color = self.default_background_color;
    }
}

impl<C> CanvasWriter<'_, C>
where
    C: Canvas + DrawTarget<Color = Rgb888>,
    <C as DrawTarget>::Error: core::fmt::Debug,
{
    /// Write a single character to the screen.
    pub fn print_char(&mut self, c: char) {
        // Print char to pos
        let mut c_buf: [u8; 4] = [0; 4];
        let text: &str = c.encode_utf8(&mut c_buf);

        Text::with_text_style(
            text,
            self.cursor,
            MonoTextStyle::new(&self.font, self.text_color),
            TextStyleBuilder::new()
                .alignment(Alignment::Left)
                .baseline(Baseline::Top)
                .line_height(self.line_height)
                .build(),
        )
        .draw(&mut self.canvas)
        .unwrap();

        self.advance_cursor();
    }
}

impl<C> Write for CanvasWriter<'_, C>
where
    C: Canvas + DrawTarget<Color = Rgb888>,
    <C as DrawTarget>::Error: core::fmt::Debug,
{
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let mut chars = s.chars();

        while let Some(c) = chars.next() {
            match c {
                '\n' => self.new_line(),
                '\r' => self.carriage_return(),
                '\x1b' => self.handle_ansi_ctrl_seq(&mut chars).map_err(|e| {
                    if self.log_errors {
                        log::error!("Failed to write to canvas: {e}");
                    }
                    core::fmt::Error
                })?,
                c => self.print_char(c),
            }
        }

        Ok(())
    }
}
