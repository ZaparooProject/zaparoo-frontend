// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The `zaparoo_scanout` kernel module's device: the layout handshake both
// presenters make before mapping anything, the mappings themselves, and the
// damage box they copy by. Written against the module's UAPI header in the
// Menu fork (`kernel/scanout-slots/zaparoo_scanout_uapi.h`).

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

const DEVICE_PATH: &str = "/dev/zaparoo-scanout";
pub(super) const SLOT_COUNT: usize = 2;
const ABI_VERSION: u32 = 2;

// Public Zaparoo scanout ABI v2: fixed-width layout, distinct ioctl namespace.
#[repr(C)]
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub abi_version: u32,
    pub slot_count: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub max_stride_bytes: u32,
    pub slot_capacity_bytes: u32,
    pub map_bytes: u32,
    pub flags: u32,
    pub slots: [[u32; 2]; SLOT_COUNT], // physical_address, mmap_offset_bytes
    pub native_control_offset_bytes: u32,
    pub native_control_bytes: u32,
    pub native_pixels_offset_bytes: u32,
    pub native_pixels_bytes: u32,
}

// _IOR('Z', nr, T): READ << 30 | size << 16 | 'Z' << 8 | nr.
pub(super) const GET_LAYOUT: libc::Ioctl =
    (2 << 30) | ((size_of::<Layout>() as libc::Ioctl) << 16) | (0x5A << 8) | 0x01;
const WAIT_NATIVE_VBLANK: libc::Ioctl =
    (2 << 30) | ((size_of::<u32>() as libc::Ioctl) << 16) | (0x5A << 8) | 0x02;

pub(super) fn expected_layout() -> Layout {
    Layout {
        abi_version: ABI_VERSION,
        slot_count: 2,
        max_width: 1920,
        max_height: 1080,
        max_stride_bytes: 3840,
        slot_capacity_bytes: 4_147_200,
        map_bytes: 4_149_248,
        // Write-combined, exclusive mapping-lifetime owner, native video
        // window, native vertical-sync wait.
        flags: 0xF,
        slots: [[0x2300_0000, 0], [0x2340_0000, 8_294_400]],
        native_control_offset_bytes: 16_588_800,
        native_control_bytes: 0x1000,
        native_pixels_offset_bytes: 24_883_200,
        native_pixels_bytes: 0x002F_F000,
    }
}

/// Opens the device and checks its layout against the one this build was
/// written for. Any difference is a refusal: the mappings below trust it.
pub(super) fn open_device() -> Result<(File, Layout), slint::PlatformError> {
    let err = slint::PlatformError::Other;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(DEVICE_PATH)
        .map_err(|e| err(format!("open {DEVICE_PATH}: {e} (module not loaded?)")))?;
    let mut layout = Layout::default();
    // SAFETY: GET_LAYOUT fills the layout struct; fd is valid.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), GET_LAYOUT, &raw mut layout) };
    if rc != 0 {
        return Err(err(format!(
            "scanout-slots GET_LAYOUT failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    if layout != expected_layout() {
        return Err(err(format!(
            "scanout-slots ABI mismatch: version {} slots {}",
            layout.abi_version, layout.slot_count
        )));
    }
    Ok((file, layout))
}

/// One exact-length shared mapping of the device, unmapped on drop. The
/// module's mmap handler picks the region from the file offset and applies
/// its own page attributes.
pub(super) struct Mapping {
    ptr: *mut u8,
    len: usize,
}

impl Mapping {
    pub(super) fn new(
        file: &File,
        offset: u32,
        len: usize,
        what: &str,
    ) -> Result<Self, slint::PlatformError> {
        // SAFETY: mapping one region exactly as the module's handler
        // requires; the fd is valid and the kernel picks the address.
        let p = unsafe {
            libc::mmap64(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                i64::from(offset),
            )
        };
        if p == libc::MAP_FAILED {
            return Err(slint::PlatformError::Other(format!(
                "mmap scanout {what}: {}",
                std::io::Error::last_os_error()
            )));
        }
        Ok(Self { ptr: p.cast(), len })
    }

    pub(super) const fn ptr(&self) -> *mut u8 {
        self.ptr
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this value owns exactly `len` bytes mapped at `ptr`.
        unsafe {
            libc::munmap(self.ptr.cast(), self.len);
        }
    }
}

/// Blocks until the native raster's next vertical sync. An error means no
/// sync arrived inside the module's timeout, or it cannot deliver one.
pub(super) fn wait_native_vblank(file: &File) -> std::io::Result<()> {
    let mut count = 0_u32;
    // SAFETY: the ioctl writes one u32 through a pointer valid for the call.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), WAIT_NATIVE_VBLANK, &raw mut count) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Damage bounding box in buffer pixels, inclusive-exclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Damage {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Damage {
    pub(super) const EMPTY: Self = Self {
        x0: u32::MAX,
        y0: u32::MAX,
        x1: 0,
        y1: 0,
    };
    pub(super) fn union(self, other: Self) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }
    pub(super) fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
}

#[cfg(test)]
mod tests {
    use super::{expected_layout, Damage, Layout, GET_LAYOUT, WAIT_NATIVE_VBLANK};

    #[test]
    #[allow(
        clippy::cast_sign_loss,
        clippy::unnecessary_cast,
        reason = "ioctl request codes are compared as their raw 32-bit pattern"
    )]
    fn layout_and_requests_match_the_v2_uapi() {
        let layout = expected_layout();
        assert_eq!(size_of::<Layout>(), 64);
        assert_eq!(layout.abi_version, 2);
        assert_eq!(GET_LAYOUT as u32, 0x8040_5a01);
        assert_eq!(WAIT_NATIVE_VBLANK as u32, 0x8004_5a02);
        // The native mappings tile Menu's 3 MiB window: one control page,
        // then both frame slots.
        assert_eq!(
            layout.native_control_bytes + layout.native_pixels_bytes,
            0x0030_0000
        );
        for offset in [
            layout.slots[1][1],
            layout.native_control_offset_bytes,
            layout.native_pixels_offset_bytes,
        ] {
            assert_eq!(offset % 4096, 0);
        }
    }

    #[test]
    fn damage_unions_to_a_bounding_box_and_empty_is_the_identity() {
        let a = Damage {
            x0: 4,
            y0: 8,
            x1: 10,
            y1: 12,
        };
        let b = Damage {
            x0: 1,
            y0: 9,
            x1: 6,
            y1: 20,
        };
        assert!(Damage::EMPTY.is_empty());
        assert_eq!(Damage::EMPTY.union(a), a);
        assert_eq!(
            a.union(b),
            Damage {
                x0: 1,
                y0: 8,
                x1: 10,
                y1: 20
            }
        );
    }
}
