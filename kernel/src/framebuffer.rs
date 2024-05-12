use bootloader_api::info::{FrameBuffer, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb888, RgbColor},
    Pixel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

impl Position {
    pub const fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
}

/// An 8-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
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
pub fn set_pixel_in(framebuffer: &mut FrameBuffer, position: Position, color: Color) {
    let info = framebuffer.info();

    // calculate offset to first byte of pixel
    let byte_offset = {
        let line_offset = position.y * info.stride;
        let pixel_offset = line_offset + position.x;
        pixel_offset * info.bytes_per_pixel // convert to byte offset
    };

    // set pixel based on color format
    let pixel_buffer = &mut framebuffer.buffer_mut()[byte_offset..];
    match info.pixel_format {
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
    framebuffer: &'f mut FrameBuffer,
}

impl<'f> Display<'f> {
    pub fn new(framebuffer: &'f mut FrameBuffer) -> Display {
        Display { framebuffer }
    }

    fn draw_pixel(&mut self, Pixel(coordinates, color): Pixel<Rgb888>) {
        // ignore any out-of-bounds pixels
        let (width, height) = {
            let info = self.framebuffer.info();
            (info.width, info.height)
        };

        let (x, y) = {
            let c: (i32, i32) = coordinates.into();
            (c.0 as usize, c.1 as usize)
        };

        if (0..width).contains(&x) && (0..height).contains(&y) {
            let color = Color::rgb(color.r(), color.g(), color.b());
            set_pixel_in(self.framebuffer, Position::new(x, y), color)
        }
    }
}

impl<'f> From<&'f mut FrameBuffer> for Display<'f> {
    fn from(framebuffer: &'f mut FrameBuffer) -> Self {
        Self::new(framebuffer)
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
        let info = self.framebuffer.info();

        Size::new(info.width as u32, info.height as u32)
    }
}
