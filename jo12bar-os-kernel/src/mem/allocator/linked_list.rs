//! Provides [LinkedListAllocator], a basic kernel allocator that keeps track
//! of free regions with a linked list.
//!
//! This is basically a duplicate of the
//! [`linked_list_allocator`](https://crates.io/crates/linked_list_allocator)
//! crate, but using a preemptable [TicketLock] instead of a spin lock for
//! better contention behaviour (at a slight performance penalty).

use alloc::alloc::{GlobalAlloc, Layout};
use core::{
    mem::{self, MaybeUninit},
    ptr::{self, NonNull},
};

use x86_64::VirtAddr;

use super::LockedAllocator;
use crate::prelude::*;

/// A sorted list of free memory holes. It uses the holes themselves to store the nodes.
#[derive(Debug)]
struct HoleList {
    first: Hole,
    bottom: *mut u8,
    top: *mut u8,
    pending_extend: u8,
}

#[derive(Debug)]
struct Cursor {
    prev: NonNull<Hole>,
    hole: NonNull<Hole>,
    top: *mut u8,
}

/// A block containing free memory. It points to the next hole and thus forms a linked list.
#[derive(Debug)]
struct Hole {
    size: usize,
    next: Option<NonNull<Hole>>,
}

/// Basic information about a hole.
#[derive(Debug, Clone, Copy)]
struct HoleInfo {
    addr: *mut u8,
    size: usize,
}

impl Cursor {
    fn next(mut self) -> Option<Self> {
        unsafe {
            self.hole.as_mut().next.map(|nhole| Cursor {
                prev: self.hole,
                hole: nhole,
                top: self.top,
            })
        }
    }

    fn current(&self) -> &Hole {
        unsafe { self.hole.as_ref() }
    }

    fn previous(&self) -> &Hole {
        unsafe { self.prev.as_ref() }
    }

    /// On success, returns the new allocation, and updates the linked list to
    /// accomodate any new holes and allocations. On error, returns the cursor
    /// unmodified, and makes no changes to the hole linked list.
    fn split_current(self, required_layout: Layout) -> Result<(*mut u8, usize), Self> {
        let front_padding;
        let alloc_ptr;
        let alloc_size;
        let back_padding;

        // Here we create a scope, JUST to make sure that any created references do not
        // live to the point where we start doing pointer surgery below.
        {
            let hole_size = self.current().size;
            let hole_addr_u8 = self.hole.as_ptr().cast::<u8>();
            let required_size = required_layout.size();
            let required_align = required_layout.align();

            // Quick check: If the new item is larger than the current hole, it's never gunna
            // work. Go ahead and bail early to save ourselves some math.
            if hole_size < required_size {
                return Err(self);
            }

            // Attempt to fracture the current hole into the following parts:
            // ([front_padding], allocation, [back_padding])
            //
            // The paddings are optional, and only placed if required.
            //
            // First, figure out if front padding is necessary. This would be necessary if the new
            // allocation has a larger alignment requirement than the current hole, and we didn't get
            // lucky with a current position that was well aligned-enough for the new item.
            let aligned_addr = if hole_addr_u8 == align_up(hole_addr_u8, required_align) {
                // hole already has the required alignment, no front padding is needed
                front_padding = None;
                hole_addr_u8
            } else {
                // The hole needs to be realigned. Push the "starting location" FORWARD the size
                // of a hole node, allowing for at least enough room for the hole header
                // and potentially some extra space.
                let new_start = hole_addr_u8.wrapping_add(HoleList::min_size());

                let aligned_addr = align_up(new_start, required_align);
                front_padding = Some(HoleInfo {
                    // Our new front padding will exist at the same location as the previous hole,
                    // it will just have a smaller size after we have chopped off the "tail" for
                    // the allocation.
                    addr: hole_addr_u8,
                    size: (aligned_addr as usize) - (hole_addr_u8 as usize),
                });
                aligned_addr
            };

            // Okay, now that we found space, we need to see if the decisions we just made
            // ACTUALLY fit in the previous hole space
            let allocation_end = aligned_addr.wrapping_add(required_size);
            let hole_end = hole_addr_u8.wrapping_add(hole_size);

            if allocation_end > hole_end {
                // hole is too small
                return Err(self);
            }

            // Yes! We have successfully placed our allocation as well.
            alloc_ptr = aligned_addr;
            alloc_size = required_size;

            // Okay, time to move onto the back padding.
            let back_padding_size = hole_end as usize - allocation_end as usize;
            back_padding = if back_padding_size == 0 {
                None
            } else {
                // NOTE: Because we always use `HoleList::align_layout`, the size of
                // the new allocation is always "rounded up" to cover any partial gaps that
                // would have occurred. For this reason, we DON'T need to "round up"
                // to account for an unaligned hole spot.
                let hole_layout = Layout::new::<Hole>();
                let back_padding_start = align_up(allocation_end, hole_layout.align());
                let back_padding_end = back_padding_start.wrapping_add(hole_layout.size());

                // Will the proposed new back padding actually fit in the old hole slot?
                if back_padding_end <= hole_end {
                    // Yes, it does! Place a back padding node
                    Some(HoleInfo {
                        addr: back_padding_start,
                        size: back_padding_size,
                    })
                } else {
                    // No, it does not. We don't want to leak any heap bytes, so we
                    // consider this hole unsuitable for the requested allocation.
                    return Err(self);
                }
            };
        }

        ////////////////////////////////////////////////////////////////////////////
        // This is where we actually perform surgery on the linked list.
        ////////////////////////////////////////////////////////////////////////////
        let Cursor {
            mut prev, mut hole, ..
        } = self;
        // Remove the current location from the previous node
        unsafe {
            prev.as_mut().next = None;
        }
        // Take the next node out of our current node
        let maybe_next_addr: Option<NonNull<Hole>> = unsafe { hole.as_mut().next.take() };

        // As of now, the old `Hole` is no more. We are about to replace it with one or more of
        // the front padding, the allocation, and the back padding.

        match (front_padding, back_padding) {
            (None, None) => {
                // No padding at all - we're lucky! We still need to connect the PREVIOUS node
                // to the NEXT node, if there was one.
                unsafe {
                    prev.as_mut().next = maybe_next_addr;
                }
            }

            (None, Some(singlepad)) | (Some(singlepad), None) => {
                // We have front padding OR back padding, but not both.
                //
                // Replace the old node with the new single node. We need to stitch the new node
                // into the linked list. Start by writing the padding into the proper location.
                let singlepad_ptr = singlepad.addr.cast::<Hole>();
                unsafe {
                    singlepad_ptr.write(Hole {
                        size: singlepad.size,
                        // If the old hole had a next pointer, the single padding now takes ownership
                        // of that link.
                        next: maybe_next_addr,
                    });
                }

                // Then connect the OLD previous to the NEW single padding:
                unsafe {
                    prev.as_mut().next = Some(NonNull::new_unchecked(singlepad_ptr));
                }
            }

            (Some(frontpad), Some(backpad)) => {
                // We have front padding AND back padding.
                //
                // We need to stitch them together as two nodes where there used to only be one.
                // Start with the back padding.
                let backpad_ptr = backpad.addr.cast::<Hole>();
                unsafe {
                    backpad_ptr.write(Hole {
                        size: backpad.size,
                        // If the old hole had a next pointer, the BACK padding now takes
                        // "ownership" of that link
                        next: maybe_next_addr,
                    });
                }

                // Now we emplace the front padding, and link it to both the back padding,
                // and the old previous
                let frontpad_ptr = frontpad.addr.cast::<Hole>();
                unsafe {
                    frontpad_ptr.write(Hole {
                        size: frontpad.size,
                        // We now connect the FRONT padding to the BACK padding
                        next: Some(NonNull::new_unchecked(backpad_ptr)),
                    });
                }

                // Then connect the OLD previous to the NEW FRONT padding
                unsafe {
                    prev.as_mut().next = Some(NonNull::new_unchecked(frontpad_ptr));
                }
            }
        }

        // We were successful at splitting the hole list! Hand off the allocation.
        Ok((alloc_ptr, alloc_size))
    }

    fn try_insert_back(self, node: NonNull<Hole>, bottom: *mut u8) -> Result<Self, Self> {
        // Covers the case where the new hole exists BEFORE the current pointer,
        // which only happens when previous is the stub pointer
        if node < self.hole {
            let node_u8 = node.as_ptr().cast::<u8>();
            let node_size = unsafe { node.as_ref().size };
            let hole_u8 = self.hole.as_ptr().cast::<u8>();

            assert!(
                node_u8.wrapping_add(node_size) <= hole_u8,
                "Freed node aliases existing hole! Bad free?",
            );
            debug_assert_eq!(self.previous().size, 0);

            let Cursor {
                mut prev,
                hole,
                top,
            } = self;
            unsafe {
                let mut node = check_merge_bottom(node, bottom);
                prev.as_mut().next = Some(node);
                node.as_mut().next = Some(hole);
            }
            Ok(Cursor {
                prev,
                hole: node,
                top,
            })
        } else {
            Err(self)
        }
    }

    fn try_insert_after(&mut self, mut node: NonNull<Hole>) -> Result<(), ()> {
        let node_u8 = node.as_ptr().cast::<u8>();
        let node_size = unsafe { node.as_ref().size };

        // If we have a next, does the node overlap next?
        if let Some(next) = self.current().next.as_ref() {
            if node < *next {
                let node_u8 = node_u8 as *const u8;
                assert!(
                    node_u8.wrapping_add(node_size) <= next.as_ptr().cast::<u8>(),
                    "Freed node aliases existing hole! Bad free?",
                );
            } else {
                // The new hole isn't between current and next.
                return Err(());
            }
        }

        // At this point, we either have no "next" pointer, or the hole is
        // between current and "next". The following assert can only trigger
        // if we've gotten our list out of order.
        debug_assert!(self.hole < node, "Hole list out of order?");

        let hole_u8 = self.hole.as_ptr().cast::<u8>();
        let hole_size = self.current().size;

        // Does hole overlap node?
        assert!(
            hole_u8.wrapping_add(hole_size) <= node_u8,
            "Freed node ({:?}) aliases existing hole ({:?}[{}])! Bad free?",
            node_u8,
            hole_u8,
            hole_size,
        );

        // All good! Let's insert that after.
        unsafe {
            let maybe_next = self.hole.as_mut().next.replace(node);
            node.as_mut().next = maybe_next;
        }

        Ok(())
    }

    // Merge the current node with up to n following nodes
    fn try_merge_next_n(self, max: usize) {
        let Cursor {
            prev: _,
            mut hole,
            top,
            ..
        } = self;

        for _ in 0..max {
            // Is there a next node?
            let mut next = if let Some(next) = unsafe { hole.as_mut() }.next.as_ref() {
                *next
            } else {
                // Since there is no NEXT node, we need to check whether the current
                // hole SHOULD extend to the end, but doesn't. This would happen when
                // there isn't enough remaining space to place a hole after the current
                // node's placement.
                check_merge_top(hole, top);
                return;
            };

            // Can we directly merge these? e.g. are they touching?
            //
            // NOTE: Because we always use `HoleList::align_layout`, the size of
            // the new hole is always "rounded up" to cover any partial gaps that
            // would have occurred. For this reason, we DON'T need to "round up"
            // to account for an unaligned hole spot.
            let hole_u8 = hole.as_ptr().cast::<u8>();
            let hole_sz = unsafe { hole.as_ref().size };
            let next_u8 = next.as_ptr().cast::<u8>();
            let end = hole_u8.wrapping_add(hole_sz);

            let touching = end == next_u8;

            if touching {
                let next_sz;
                let next_next;
                unsafe {
                    let next_mut = next.as_mut();
                    next_sz = next_mut.size;
                    next_next = next_mut.next.take();
                }
                unsafe {
                    let hole_mut = hole.as_mut();
                    hole_mut.next = next_next;
                    hole_mut.size += next_sz;
                }
                // Okay, we just merged the next item. DON'T move the cursor, as we can
                // just try to merge the next_next, which is now our next.
            } else {
                // Welp, not touching, can't merge. Move to the next node.
                hole = next;
            }
        }
    }
}

/// Test if a hole can be extended towards the end of an allocation region.
/// If so, increase our node size. If not, keep node as-is.
fn check_merge_top(mut node: NonNull<Hole>, top: *mut u8) {
    let node_u8 = node.as_ptr().cast::<u8>();
    let node_sz = unsafe { node.as_ref().size };

    // If this is the last node, we need to see if we need to merge to the end
    let end = node_u8.wrapping_add(node_sz);
    let hole_layout = Layout::new::<Hole>();
    if end < top {
        let next_hole_end = align_up(end, hole_layout.align()).wrapping_add(hole_layout.size());

        if next_hole_end > top {
            let offset = (top as usize) - (end as usize);
            unsafe {
                node.as_mut().size += offset;
            }
        }
    }
}

/// Test if a hole can be moved back to the bottom of an allocation region.
/// If so, create and return the new hole. If not, return the existing hole.
fn check_merge_bottom(node: NonNull<Hole>, bottom: *mut u8) -> NonNull<Hole> {
    debug_assert_eq!(bottom as usize % align_of::<Hole>(), 0);

    if bottom.wrapping_add(core::mem::size_of::<Hole>()) > node.as_ptr().cast::<u8>() {
        let offset = (node.as_ptr() as usize) - (bottom as usize);
        let size = unsafe { node.as_ref() }.size + offset;
        unsafe { make_hole(bottom, size) }
    } else {
        node
    }
}

impl HoleList {
    /// Create a new, empty `HoleList`.
    const fn new() -> Self {
        Self {
            first: Hole {
                size: 0,
                next: None,
            },
            bottom: ptr::null_mut(),
            top: ptr::null_mut(),
            pending_extend: 0,
        }
    }

    fn cursor(&mut self) -> Option<Cursor> {
        if let Some(hole) = self.first.next {
            Some(Cursor {
                hole,
                prev: NonNull::new(&mut self.first)?,
                top: self.top,
            })
        } else {
            None
        }
    }

    /// Create a new `HoleList` that contains a given hole.
    ///
    /// The `hole_addr` pointer is automatically aligned by this function, so
    /// `self.bottom` might be larger than the given `hole_addr`.
    ///
    /// The given `hole_size` must be large enough to store the required
    /// metadata, otherwise this function will panic. Depending on the
    /// alignment of the `hole_addr` pointer, the minimum size is between
    /// `2 * size_of::<usize>` and `3 * size_of::<usize>`.
    ///
    /// The usable size for allocations will be truncated to the nearest
    /// alignment of `align_of::<usize>`. Any extra bytes left at the end
    /// will be reclaimed once sufficient additional space is given to
    /// [`extend`][LinkedListAllocator::extend].
    ///
    /// # Safety
    ///
    /// This function is unsafe because it creates a hole at the given `hole_addr`.
    /// This can cause undefined behavior if this address is invalid or if memory from the
    /// `[hole_addr, hole_addr+size)` range is used somewhere else.
    pub unsafe fn new_with_hole(hole_addr: *mut u8, hole_size: usize) -> HoleList {
        assert_eq!(size_of::<Hole>(), Self::min_size());
        assert!(hole_size >= size_of::<Hole>());

        let aligned_hole_addr = align_up(hole_addr, align_of::<Hole>());
        let requested_hole_size = hole_size - ((aligned_hole_addr as usize) - (hole_addr as usize));
        let aligned_hole_size = align_down_size(requested_hole_size, align_of::<Hole>());
        assert!(aligned_hole_size >= size_of::<Hole>());

        let ptr = aligned_hole_addr as *mut Hole;
        unsafe {
            ptr.write(Hole {
                size: aligned_hole_size,
                next: None,
            });
        }

        assert_eq!(
            hole_addr.wrapping_add(hole_size),
            aligned_hole_addr.wrapping_add(requested_hole_size)
        );

        HoleList {
            first: Hole {
                size: 0,
                next: unsafe { Some(NonNull::new_unchecked(ptr)) },
            },
            bottom: aligned_hole_addr,
            top: aligned_hole_addr.wrapping_add(aligned_hole_size),
            pending_extend: (requested_hole_size - aligned_hole_size) as u8,
        }
    }

    /// Align the given layout for use with the `HoleList`.
    ///
    /// Returns a layout with size increased to fit at least [`HoleList::min_size()`]
    /// and proper alignment of a `Hole`.
    ///
    /// The [`allocate_first_fit`][HoleList::allocate_first_fit] and
    /// [`deallocate`][HoleList::deallocate] methods perform the required alignment
    /// themselves, so calling this function manually is not necessary.
    pub fn align_layout(layout: Layout) -> Layout {
        let mut size = layout.size();
        if size < Self::min_size() {
            size = Self::min_size();
        }
        let size = align_up_size(size, mem::align_of::<Hole>());
        Layout::from_size_align(size, layout.align()).unwrap()
    }

    /// Searches the list for a big enough hole.
    ///
    /// A hole is big enough if it can hold an allocation of `layout.size()` bytes with
    /// the given `layout.align()`. If such a hole is found in the list, a block of the
    /// required size is allocated from it. Then the start address of that
    /// block and the aligned layout are returned. The automatic layout alignment is required
    /// because the `HoleList` has some additional layout requirements for each memory block.
    ///
    /// This function uses the “first fit” strategy, so it uses the first hole that is big
    /// enough. Thus the runtime is in O(n) but it should be reasonably fast for small allocations.
    pub fn allocate_first_fit(&mut self, layout: Layout) -> Option<(NonNull<u8>, Layout)> {
        let aligned_layout = Self::align_layout(layout);
        let mut cursor = self.cursor()?;

        loop {
            match cursor.split_current(aligned_layout) {
                Ok((ptr, _len)) => {
                    return Some((NonNull::new(ptr)?, aligned_layout));
                }
                Err(curs) => {
                    cursor = curs.next()?;
                }
            }
        }
    }

    /// Frees the allocation given by `ptr` and `layout`.
    ///
    /// This function walks the list and inserts the given block at the correct place. If the freed
    /// block is adjacent to another free block, the blocks are merged again.
    /// This operation is in `O(n)` since the list needs to be sorted by address.
    ///
    /// [`allocate_first_fit`]: HoleList::allocate_first_fit
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by a call to the [`allocate_first_fit`] function with
    /// identical layout. Undefined behavior may occur for invalid arguments.
    /// The function performs exactly the same layout adjustments as [`allocate_first_fit`] and
    /// returns the aligned layout.
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) -> Layout {
        let aligned_layout = Self::align_layout(layout);
        deallocate(self, ptr.as_ptr(), aligned_layout.size());
        aligned_layout
    }

    /// Returns the minimal allocation size. Smaller allocations or deallocations are not allowed.
    pub fn min_size() -> usize {
        size_of::<usize>() * 2
    }

    pub(crate) unsafe fn extend(&mut self, by: usize) {
        assert!(!self.top.is_null(), "tried to extend an empty heap");

        let top = self.top;

        let dead_space = top.align_offset(align_of::<Hole>());
        debug_assert_eq!(
            0, dead_space,
            "dead space detected during extend: {} bytes. This means top was unaligned",
            dead_space
        );

        debug_assert!(
            (self.pending_extend as usize) < Self::min_size(),
            "pending extend was larger than expected"
        );

        // join this extend request with any pending (but not yet acted on) extension
        let extend_by = self.pending_extend as usize + by;

        let minimum_extend = Self::min_size();
        if extend_by < minimum_extend {
            self.pending_extend = extend_by as u8;
            return;
        }

        // only extend up to another valid boundary
        let new_hole_size = align_down_size(extend_by, align_of::<Hole>());
        let layout = Layout::from_size_align(new_hole_size, 1).unwrap();

        // instantiate the hole by forcing a deallocation on the new memory
        unsafe {
            self.deallocate(NonNull::new_unchecked(top), layout);
            self.top = top.add(new_hole_size);
        }

        // save extra bytes given to extend that weren't aligned to the hole size
        self.pending_extend = (extend_by - new_hole_size) as u8;
    }
}

unsafe fn make_hole(addr: *mut u8, size: usize) -> NonNull<Hole> {
    let hole_addr = addr.cast::<Hole>();
    debug_assert_eq!(
        addr as usize % align_of::<Hole>(),
        0,
        "Hole address not aligned!",
    );
    unsafe {
        hole_addr.write(Hole { size, next: None });
        NonNull::new_unchecked(hole_addr)
    }
}

/// Frees the allocation given by `(addr, size)`. It starts at the given hole and walks the list to
/// find the correct place (the list is sorted by address).
fn deallocate(list: &mut HoleList, addr: *mut u8, size: usize) {
    // Start off by just making this allocation a hole where it stands.
    // We'll attempt to merge it with other nodes once we figure out where
    // it should live
    let hole = unsafe { make_hole(addr, size) };

    // Now, try to get a cursor to the list - this only works if we have at least
    // one non-"dummy" hole in the list
    let cursor = if let Some(cursor) = list.cursor() {
        cursor
    } else {
        // Oh hey, there are no "real" holes at all. That means this just
        // becomes the only "real" hole! Check if this is touching the end
        // or the beginning of the allocation range
        let hole = check_merge_bottom(hole, list.bottom);
        check_merge_top(hole, list.top);
        list.first.next = Some(hole);
        return;
    };

    // First, check if we can just insert this node at the top of the list. If the
    // insertion succeeded, then our cursor now points to the NEW node, behind the
    // previous location the cursor was pointing to.
    //
    // Otherwise, our cursor will point at the current non-"dummy" head of the list
    let (cursor, n) = match cursor.try_insert_back(hole, list.bottom) {
        Ok(cursor) => {
            // Yup! It lives at the front of the list. Hooray! Attempt to merge
            // it with just ONE next node, since it is at the front of the list
            (cursor, 1)
        }
        Err(mut cursor) => {
            // Nope. It lives somewhere else. Advance the list until we find its home
            while let Err(()) = cursor.try_insert_after(hole) {
                cursor = cursor
                    .next()
                    .expect("Reached end of holes without finding deallocation hole!");
            }
            // Great! We found a home for it, our cursor is now JUST BEFORE the new
            // node we inserted, so we need to try to merge up to twice: One to combine
            // the current node to the new node, then once more to combine the new node
            // with the node after that.
            (cursor, 2)
        }
    };

    // We now need to merge up to two times to combine the current node with the next
    // two nodes.
    cursor.try_merge_next_n(n);
}

/// A kernel allocator that keeps track of free regions using a linked list.
#[derive(Debug)]
pub struct LinkedListAllocator {
    used: usize,
    /// The start of the "freelist" - a linked list of free regions of memory.
    holes: HoleList,
}

impl LinkedListAllocator {
    /// Create an empty [`LinkedListAllocator`].
    ///
    /// Should be initialized with [`LinkedListAllocator::init()`] before use.
    pub const fn new() -> Self {
        Self {
            used: 0,
            holes: HoleList::new(),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// # Safety
    /// - Caller must guarantee that the given heap bounds are valid and that
    ///   the heap is unused.
    /// - This method must be called only once.
    pub unsafe fn init(&mut self, heap_start: VirtAddr, heap_size: u64) {
        self.used = 0;
        unsafe {
            self.holes = HoleList::new_with_hole(heap_start.as_mut_ptr(), heap_size as _);
        }
    }

    /// Initialize an empty heap with provided memory.
    ///
    /// The caller is responsible for procuring a region of raw memory that may be utilized by the
    /// allocator. This might be done via any method such as (unsafely) taking a region from the
    /// program's memory, from a mutable static, or by allocating and leaking such memory from
    /// another allocator.
    ///
    /// The latter approach may be especially useful if the underlying allocator does not perform
    /// deallocation (e.g. a simple bump allocator). Then the overlaid linked-list-allocator can
    /// provide memory reclamation.
    ///
    /// The usable size for allocations will be truncated to the nearest
    /// alignment of `align_of::<usize>`. Any extra bytes left at the end
    /// will be reclaimed once sufficient additional space is given to
    /// [`extend`][Heap::extend].
    ///
    /// # Panics
    ///
    /// This method panics if the heap is already initialized.
    ///
    /// It also panics when the length of the given `mem` slice is not large enough to
    /// store the required metadata. Depending on the alignment of the slice, the minimum
    /// size is between `2 * size_of::<usize>` and `3 * size_of::<usize>`.
    pub fn init_from_slice(&mut self, mem: &'static mut [MaybeUninit<u8>]) {
        assert!(
            self.bottom().is_null(),
            "The heap has already been initialized."
        );
        let size = mem.len();
        let address = mem.as_mut_ptr().cast::<u8>();
        // SAFETY: All initialization requires the bottom address to be valid, which implies it
        // must not be 0. Initially the address is 0. The assertion above ensures that no
        // initialization had been called before.
        // The given address and size is valid according to the safety invariants of the mutable
        // reference handed to us by the caller.
        unsafe { self.init(VirtAddr::from_ptr(address), size as _) }
    }

    /// Creates a new heap with the given `bottom` and `size`.
    ///
    /// The `heap_bottom` pointer is automatically aligned, so the [`bottom()`][Self::bottom]
    /// method might return a pointer that is larger than `heap_bottom` after construction.
    ///
    /// The given `heap_size` must be large enough to store the required
    /// metadata, otherwise this function will panic. Depending on the
    /// alignment of the `hole_addr` pointer, the minimum size is between
    /// `2 * size_of::<usize>` and `3 * size_of::<usize>`.
    ///
    /// The usable size for allocations will be truncated to the nearest
    /// alignment of `align_of::<usize>`. Any extra bytes left at the end
    /// will be reclaimed once sufficient additional space is given to
    /// [`extend`][Heap::extend].
    ///
    /// # Safety
    ///
    /// The bottom address must be valid and the memory in the
    /// `[heap_bottom, heap_bottom + heap_size)` range must not be used for anything else.
    /// This function is unsafe because it can cause undefined behavior if the given address
    /// is invalid.
    ///
    /// The provided memory range must be valid for the `'static` lifetime.
    pub unsafe fn new_with_address_and_size(heap_bottom: *mut u8, heap_size: usize) -> Self {
        Self {
            used: 0,
            holes: unsafe { HoleList::new_with_hole(heap_bottom, heap_size) },
        }
    }

    /// Creates a new heap from a slice of raw memory.
    ///
    /// This is a convenience function that has the same effect as calling
    /// [`init_from_slice`] on an empty heap. All the requirements of `init_from_slice`
    /// apply to this function as well.
    pub fn from_slice(mem: &'static mut [MaybeUninit<u8>]) -> Self {
        let size = mem.len();
        let address = mem.as_mut_ptr().cast();
        // SAFETY: The given address and size is valid according to the safety invariants of the
        // mutable reference handed to us by the caller.
        unsafe { Self::new_with_address_and_size(address, size) }
    }

    /// Allocates a chunk of the given size with the given alignment. Returns a pointer to the
    /// beginning of that chunk if it was successful. Else it returns `None`.
    /// This function scans the list of free memory blocks and uses the first block that is big
    /// enough. The runtime is in O(n) where n is the number of free blocks, but it should be
    /// reasonably fast for small allocations.
    pub fn allocate_first_fit(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let (ptr, aligned_layout) = self.holes.allocate_first_fit(layout)?;
        self.used += aligned_layout.size();
        Some(ptr)
    }

    /// Frees the given allocation. `ptr` must be a pointer returned
    /// by a call to the `allocate_first_fit` function with identical size and alignment.
    ///
    /// This function walks the list of free memory blocks and inserts the freed block at the
    /// correct place. If the freed block is adjacent to another free block, the blocks are merged
    /// again. This operation is in `O(n)` since the list needs to be sorted by address.
    ///
    /// # Safety
    ///
    /// `ptr` must be a pointer returned by a call to the [`allocate_first_fit`] function with
    /// identical layout. Undefined behavior may occur for invalid arguments.
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        unsafe {
            self.used -= self.holes.deallocate(ptr, layout).size();
        }
    }

    /// Returns the bottom address of the heap.
    ///
    /// The bottom pointer is automatically aligned, so the returned pointer
    /// might be larger than the bottom pointer used for initialization.
    pub fn bottom(&self) -> *mut u8 {
        self.holes.bottom
    }

    /// Returns the size of the heap.
    ///
    /// This is the size the heap is using for allocations, not necessarily the
    /// total amount of bytes given to the heap. To determine the exact memory
    /// boundaries, use [`bottom`][Self::bottom] and [`top`][Self::top].
    pub fn size(&self) -> usize {
        unsafe { self.holes.top.offset_from(self.holes.bottom) as usize }
    }

    /// Return the top address of the heap.
    ///
    /// Note: The heap may choose to not use bytes at the end for allocations
    /// until there is enough room for metadata, but it still retains ownership
    /// over memory from [`bottom`][Self::bottom] to the address returned.
    pub fn top(&self) -> *mut u8 {
        unsafe { self.holes.top.add(self.holes.pending_extend as usize) }
    }

    /// Returns the size of the used part of the heap
    pub fn used(&self) -> usize {
        self.used
    }

    /// Returns the size of the free part of the heap
    pub fn free(&self) -> usize {
        self.size() - self.used
    }

    /// Extends the size of the heap by creating a new hole at the end.
    ///
    /// Small extensions are not guaranteed to grow the usable size of
    /// the heap. In order to grow the Heap most effectively, extend by
    /// at least `2 * size_of::<usize>`, keeping the amount a multiple of
    /// `size_of::<usize>`.
    ///
    /// Calling this method on an uninitialized Heap will panic.
    ///
    /// # Safety
    ///
    /// The amount of data given in `by` MUST exist directly after the original
    /// range of data provided when constructing the [Heap]. The additional data
    /// must have the same lifetime of the original range of data.
    ///
    /// Even if this operation doesn't increase the [usable size][`Self::size`]
    /// by exactly `by` bytes, those bytes are still owned by the Heap for
    /// later use.
    pub unsafe fn extend(&mut self, by: usize) {
        unsafe {
            self.holes.extend(by);
        }
    }
}

// this is fine because there will only ever be a single allocator, and nothing
// will be able to gain actual references to it.
unsafe impl Send for LinkedListAllocator {}

unsafe impl GlobalAlloc for LockedAllocator<LinkedListAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.lock()
            .allocate_first_fit(layout)
            .map_or(ptr::null_mut(), |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { self.lock().deallocate(NonNull::new_unchecked(ptr), layout) }
    }
}

impl Default for LinkedListAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Align downwards. Returns the greatest x with alignment `align`
/// so that x <= addr. The alignment must be a power of 2.
const fn align_down_size(size: usize, align: usize) -> usize {
    if align.is_power_of_two() {
        size & !(align - 1)
    } else if align == 0 {
        size
    } else {
        panic!("`align` must be a power of 2");
    }
}

const fn align_up_size(size: usize, align: usize) -> usize {
    align_down_size(size + align - 1, align)
}

/// Align upwards. Returns the smallest x with alignment `align`
/// so that x >= addr. The alignment must be a power of 2.
fn align_up(addr: *mut u8, align: usize) -> *mut u8 {
    let offset = addr.align_offset(align);
    addr.wrapping_add(offset)
}
