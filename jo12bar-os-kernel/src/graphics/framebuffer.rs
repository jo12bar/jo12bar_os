//! Framebuffer utilities.

use core::slice;

use bootloader_api::info::{FrameBuffer as BootFrameBuffer, FrameBufferInfo, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget, geometry::OriginDimensions, pixelcolor::Rgb888, prelude::*,
};
use spinning_top::Spinlock;
use x86_64::VirtAddr;

use super::canvas::Canvas;

/// The main hardware-backed framebuffer. This can be taken, at which point it
/// will be `None`.
///
/// TODO: Investigate using a [ticket lock](https://en.wikipedia.org/wiki/Ticket_lock)
/// instead of a spinlock.
pub static HARDWARE_FRAMEBUFFER: Spinlock<Option<Framebuffer>> = Spinlock::new(None);

/// Different memory sources for the [`Framebuffer`].
#[derive(Debug)]
enum FramebufferSource {
    /// Framebuffer is backed by the hardware framebuffer.
    HardwareBuffer,
    // TODO: Implement memory-backed framebuffers.
    // /// Framebuffer is backed by normal mapped memory.
    // Owned(Mapped<GuardedPages<Size4KiB>>),
    /// Framebuffer is dropped.
    #[allow(dead_code)]
    Dropped,
}

impl FramebufferSource {
    // TODO: Make this return something like Option<Mapped<GuardedPages<4KiB>>>
    // when implementing memory-backed framebuffers.
    fn drop(&mut self) -> Option<()> {
        match self {
            FramebufferSource::HardwareBuffer => None,
            // FramebufferSource::Owned(pages) => {
            //     let pages = *pages;
            //     *self = FramebufferSource::Dropped;
            //     Some(pages)
            // }
            FramebufferSource::Dropped => None,
        }
    }
}

/// A framebuffer for rendering to the screen.
#[derive(Debug)]
pub struct Framebuffer {
    /// The framebuffer's start address.
    pub(super) start: VirtAddr,

    /// The source of the framebuffer's memory.
    source: FramebufferSource,

    /// Information about the framebuffer's memory layout.
    pub info: FrameBufferInfo,
}

impl Framebuffer {
    // TODO: Implement memory-backed framebuffers
    // /// Allocates a new memory backed framebuffer
    // pub fn alloc_new(info: FrameBufferInfo) -> Result<Self, MemError> {
    //     let page_count = (info.byte_len as u64 + Size4KiB::SIZE - 1) / Size4KiB::SIZE;

    //     let pages = PageAllocator::get_kernel_allocator()
    //         .lock()
    //         .allocate_guarded_pages(page_count, true, true)?;

    //     let pages = Unmapped(pages);
    //     let mapped_pages = pages.alloc_and_map()?;
    //     let start = mapped_pages.0.start_addr();

    //     let source = FramebufferSource::Owned(mapped_pages);

    //     Ok(Framebuffer {
    //         start,
    //         source,
    //         info,
    //     })
    // }

    /// Create a new framebuffer at the given `vaddr`.
    ///
    /// # Safety
    /// `vaddr` must be a valid memory location with a lifetime of _at least_ the
    /// result of this function, and cannot be accessed in any other way.
    pub unsafe fn new_at_virt_addr(vaddr: VirtAddr, info: FrameBufferInfo) -> Self {
        Framebuffer {
            start: vaddr,
            source: FramebufferSource::HardwareBuffer,
            info,
        }
    }

    /// Get shared immutable access to the underlying buffer.
    pub fn buffer(&self) -> &[u8] {
        // Safety: buffer start + byte_len is memory owned by this framebuffer.
        unsafe { slice::from_raw_parts(self.start.as_ptr(), self.info.byte_len) }
    }

    /// Get exclusive mutable access to the underlying buffer.
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        // Safety: buffer start + byte_len is memory owned by this framebuffer.
        unsafe { slice::from_raw_parts_mut(self.start.as_mut_ptr(), self.info.byte_len) }
    }
}

impl From<BootFrameBuffer> for Framebuffer {
    fn from(fb: BootFrameBuffer) -> Self {
        // TODO use VirtAddr::from_slice once that is available
        let start = VirtAddr::new(fb.buffer() as *const [u8] as *const u8 as u64);

        // Safety: `start` points to valid framebuffer memory, since we got it from
        // the bootloader's framebuffer.
        unsafe { Self::new_at_virt_addr(start, fb.info()) }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        if let Some(_pages) = self.source.drop() {
            todo!("memory-backed framebuffers");
            // unsafe {
            //     // Safety: after drop, there are no ways to access the fb memory
            //     pages
            //         .unmap_and_free()
            //         .expect("failed to deallco framebuffer");
            // }
        }
    }
}

impl Canvas for Framebuffer {
    fn supports_scrolling() -> bool {
        true
    }

    fn set_pixel(&mut self, x: u32, y: u32, c: Rgb888) {
        let info = &self.info;
        let pos =
            info.bytes_per_pixel * info.stride * y as usize + info.bytes_per_pixel * x as usize;
        let format = info.pixel_format;
        set_pixel_at_pos(self.buffer_mut(), pos, c, format);
    }

    fn scroll(
        &mut self,
        height: i32,
        clear_color: Rgb888,
    ) -> Result<(), super::canvas::ScrollingNotSupportedError> {
        if height == 0 {
            return Ok(());
        }

        let lines_to_move = self.height() as usize - height.unsigned_abs() as usize;

        let bytes_per_line = self.info.stride * self.info.bytes_per_pixel;
        let (start, dest, clear_start): (usize, usize, u32) = if height.is_positive() {
            // Move every line up by height pixels. Therefore, we copy starting
            // from the nth line and copy into the 0th line.
            (height as usize * bytes_per_line, 0, lines_to_move as u32)
        } else {
            // Move every line down by height pixels. Therefore, we copy starting
            // from the 0th line and copy into the nth line.
            (0, height.unsigned_abs() as usize * bytes_per_line, 0)
        };
        let src = start..(start + lines_to_move * bytes_per_line);

        self.buffer_mut().copy_within(src, dest);

        // Clear the freed up lines
        for line in 0..height.unsigned_abs() {
            let y = clear_start + line;
            for x in 0..self.width() {
                self.set_pixel(x, y, clear_color);
            }
        }

        Ok(())
    }
}

impl DrawTarget for Framebuffer {
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
            self.set_pixel(pixel.0.x as _, pixel.0.y as _, pixel.1);
        }

        Ok(())
    }
}

impl OriginDimensions for Framebuffer {
    fn size(&self) -> Size {
        Size::new(self.info.width as u32, self.info.height as u32)
    }
}

// Sets the pixel at `index` to `color`.
///
/// `index` is not the n'th pixel but the index in the `buffer` where the pixel
/// starts.
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
fn set_pixel_at_pos(buffer: &mut [u8], index: usize, color: Rgb888, pixel_format: PixelFormat) {
    let (r, g, b) = (color.r(), color.g(), color.b());
    match pixel_format {
        PixelFormat::Rgb => {
            buffer[index] = r;
            buffer[index + 1] = g;
            buffer[index + 2] = b;
        }

        PixelFormat::Bgr => {
            buffer[index] = b;
            buffer[index + 1] = g;
            buffer[index + 2] = r;
        }

        PixelFormat::U8 => {
            let grey =
                (0.3 * (r as f32) + 0.59 * (g as f32) + 0.11 * (b as f32)).clamp(0.0, 255.0) as u8;
            buffer[index] = grey;
        }

        other => panic!("unknown pixel format {other:?}"),
    }
}

/// Module containing startup/panic recovery functionality for the [`Framebuffer`][super::Framebuffer].
pub mod startup {
    use bootloader_api::info::{FrameBuffer, FrameBufferInfo, Optional};
    use x86_64::VirtAddr;

    use crate::boot_info;

    /// The start addr of the hardware framebuffer. Used during panic to recreate the fb
    pub static mut HARDWARE_FRAMEBUFFER_START_INFO: Option<(VirtAddr, FrameBufferInfo)> = None;

    /// Extracts the hardware framebuffer from the boot info.
    ///
    /// # Safety
    ///
    /// This is racy and must only be called while only a single execution has access.
    pub unsafe fn take_boot_framebuffer() -> Option<FrameBuffer> {
        let boot_info = unsafe { boot_info() };
        let fb = core::mem::replace(&mut boot_info.framebuffer, Optional::None);
        fb.into_option()
    }
}
