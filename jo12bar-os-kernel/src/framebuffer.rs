//! Utilities for displaying things on the framebuffer, including drawing and logging.
//!
//! [`Display`] allows for drawing to the framebuffer using the [`embedded_graphics`]
//! crate. [`LockedDisplay`] wraps the [`Display`] in a [`Spinlock`], and implements
//! [`log::Log`] so it can be used as a logging target.

use core::fmt::{self, Write};
use core::ops::Deref;

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    mono_font::{ascii::FONT_8X13, MonoTextStyle},
    pixelcolor::{Rgb888, RgbColor},
    primitives::Rectangle,
    text::{
        renderer::TextRenderer, Alignment, Baseline, LineHeight, Text, TextStyle, TextStyleBuilder,
    },
    Drawable, Pixel,
};
use spinning_top::Spinlock;

/// A 2D position in the [`FrameBuffer`].
///
/// (0, 0) is in the top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Horizontal position. Increases going right, decreases going left.
    pub x: usize,
    /// Vertical position. Increases going down, decreases going up.
    pub y: usize,
}

impl Position {
    /// Instantiate a new [`Position`].
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

/// An 8-bit RGB color.
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    /// Instantiate a new 8-bit RGB color.
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

/// Set a pixel at some `position` in a `framebuffer` to an 8-bit RGB `color`.
///
/// If the framebuffer is greyscale, then the 3 components of the `color` will
/// averaged with weights described under
/// [*"3.3. Luminosity Method"* on this page](https://www.baeldung.com/cs/convert-rgb-to-grayscale#3-luminosity-method):
///
/// > The best method is the luminosity method that successfully solves the
/// > problems of previous methods.
/// >
/// > Based on the aforementioned observations, we should take a weighted
/// > average of the components. **The contribution of blue to the final value
/// > should decrease, and the contribution of green should increase. After some
/// > experiments and more in-depth analysis, researchers have concluded in the
/// > equation below:**
/// >
/// > ```text
/// > grayscale = 0.3 * R + 0.59 * G + 0.11 * B
/// > ```
///
/// Custom greyscale transforms are not yet supported.
pub fn set_pixel_in(
    framebuffer: &mut [u8],
    fb_info: &FrameBufferInfo,
    position: Position,
    color: Color,
) {
    // calculate offset to first byte of pixel
    let byte_offset = {
        let line_offset = position.y * fb_info.stride;
        let pixel_offset = line_offset + position.x;
        pixel_offset * fb_info.bytes_per_pixel // convert to byte offset
    };

    // set pixel based on color format
    let pixel_buffer = &mut framebuffer[byte_offset..];
    match fb_info.pixel_format {
        PixelFormat::Rgb => {
            pixel_buffer[0] = color.red;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.blue;
        }

        PixelFormat::Bgr => {
            pixel_buffer[0] = color.blue;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.red;
        }

        PixelFormat::U8 => {
            // use a simple average-based greyscale transform
            let grey = (0.3 * (color.red as f32)
                + 0.59 * (color.green as f32)
                + 0.11 * (color.blue as f32))
                .clamp(0.0, 255.0) as u8;
            pixel_buffer[0] = grey;
        }

        other => panic!("unknown pixel format {other:?}"),
    }
}

/// A wrapper struct for [`FrameBuffer`]s to allow using the [`embedded_graphics`]
/// crate to draw on them.
pub struct Display<'f> {
    framebuffer: &'f mut [u8],
    framebuffer_info: FrameBufferInfo,
    log_character_style: MonoTextStyle<'static, Rgb888>,
    log_text_style: TextStyle,
    log_bounds: Rectangle,
    log_pos: Point,
}

impl<'f> Display<'f> {
    /// Wrap a mutable reference to a [`FrameBuffer`], allowing for drawing with
    /// [`embedded_graphics`].
    pub fn new(framebuffer: &'f mut [u8], framebuffer_info: FrameBufferInfo) -> Display {
        let (fb_width, fb_height) = (framebuffer_info.width, framebuffer_info.height);

        let log_character_style = MonoTextStyle::new(&FONT_8X13, Rgb888::WHITE);
        let log_text_style = TextStyleBuilder::new()
            .alignment(Alignment::Left)
            .baseline(Baseline::Top)
            .line_height(LineHeight::Percent(100))
            .build();
        let log_bounds = Rectangle::new(Point::zero(), Size::new(fb_width as _, fb_height as _));

        Display {
            framebuffer,
            framebuffer_info,
            log_character_style,
            log_text_style,
            log_bounds,
            log_pos: Point::new(0, 0),
        }
    }

    fn draw_pixel(&mut self, Pixel(coordinates, color): Pixel<Rgb888>) {
        // ignore any out-of-bounds pixels
        let (width, height) = {
            let info = self.framebuffer_info;
            (info.width, info.height)
        };

        let (x, y) = {
            let c: (i32, i32) = coordinates.into();
            (c.0 as usize, c.1 as usize)
        };

        if (0..width).contains(&x) && (0..height).contains(&y) {
            let color = Color::rgb(color.r(), color.g(), color.b());
            set_pixel_in(
                self.framebuffer,
                &self.framebuffer_info,
                Position::new(x, y),
                color,
            )
        }
    }

    fn log_newline(&mut self) {
        let abs_line_height = self
            .log_text_style
            .line_height
            .to_absolute(self.log_character_style.line_height());

        self.log_pos.y += abs_line_height as i32;
        self.log_carriage_return();
    }

    fn log_carriage_return(&mut self) {
        self.log_pos.x = 0;
    }

    fn log_width(&mut self) -> u32 {
        self.log_bounds.size.width
    }

    fn log_height(&mut self) -> u32 {
        self.log_bounds.size.height
    }

    fn write_log_char(&mut self, c: char) {
        let abs_line_height = self
            .log_text_style
            .line_height
            .to_absolute(self.log_character_style.line_height());

        let char_width = self.log_character_style.font.character_size.width;

        match c {
            '\n' => self.log_newline(),
            '\r' => self.log_carriage_return(),
            c => {
                let new_xpos = self.log_pos.x + char_width as i32;
                if new_xpos >= self.log_width() as i32 {
                    self.log_newline()
                }

                let new_ypos = self.log_pos.y + abs_line_height as i32;
                if new_ypos >= self.log_height() as i32 {
                    self.clear(Rgb888::BLACK).unwrap();
                    self.log_pos = Point::zero();
                }

                self.write_log_rendered_char(c);
            }
        }
    }

    fn write_log_rendered_char(&mut self, c: char) {
        let mut c_buf: [u8; 4] = [0; 4];
        let text: &str = c.encode_utf8(&mut c_buf);

        Text::with_text_style(
            text,
            self.log_pos,
            self.log_character_style,
            self.log_text_style,
        )
        .draw(self)
        .unwrap();

        let char_width = self.log_character_style.font.character_size.width;
        let char_horiz_gap = self.log_character_style.font.character_spacing;

        self.log_pos.x += (char_width + char_horiz_gap) as i32;
    }
}

impl<'f> DrawTarget for Display<'f> {
    type Color = Rgb888;

    /// Drawing operations can never fail.
    ///
    /// (more accurately, we have no way to detect failures)
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for pixel in pixels.into_iter() {
            self.draw_pixel(pixel);
        }

        Ok(())
    }
}

impl<'f> OriginDimensions for Display<'f> {
    fn size(&self) -> Size {
        let info = self.framebuffer_info;

        Size::new(info.width as u32, info.height as u32)
    }
}

impl<'f> Write for Display<'f> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_log_char(c);
        }
        Ok(())
    }
}

/// A [`Display`] locked behind a [`Spinlock`].
#[repr(transparent)]
pub struct LockedDisplay<'f> {
    inner: Spinlock<Display<'f>>,
}

impl<'f> LockedDisplay<'f> {
    /// Lock a [`Display`] behind a [`Spinlock`], allowing for synchronized drawing
    /// to a [`FrameBuffer`].
    pub fn new(display: Display<'f>) -> LockedDisplay<'f> {
        LockedDisplay {
            inner: Spinlock::new(display),
        }
    }
}

impl<'f> Deref for LockedDisplay<'f> {
    type Target = Spinlock<Display<'f>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl log::Log for LockedDisplay<'_> {
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        use x86_64::instructions::interrupts;

        interrupts::without_interrupts(|| {
            let mut display = self.inner.lock();
            writeln!(display, "[{:<5}] {}", record.level(), record.args()).unwrap();
        });
    }

    fn flush(&self) {}
}
