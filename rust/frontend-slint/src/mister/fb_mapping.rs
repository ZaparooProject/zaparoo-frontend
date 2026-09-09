// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! `MiSTer_fb` kernels without `fb_mmap` return ENODEV; map their advertised
//! scanout range through /dev/mem instead.
//! Slint already stages rendering in cached RAM; only the scanout mapping changes.
use super::fb0::FbFixScreeninfo;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};

pub(super) struct FbMapping {
    ptr: *mut libc::c_void,
    len: usize,
}

impl FbMapping {
    pub fn open(file: &File, fix: &FbFixScreeninfo) -> io::Result<Self> {
        let len = fix.smem_len as usize;
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "framebuffer reports zero memory length",
            ));
        }
        map_or_fallback(
            map(file, len, 0),
            || {
                std::env::var_os("ZAPAROO_FB_FALLBACK").is_none_or(|mode| mode != "off")
                    && file.metadata().is_ok_and(|meta| {
                        meta.file_type().is_char_device() && libc::major(meta.rdev()) == 29
                    })
            },
            || {
                // SAFETY: sysconf has no pointer arguments or process-side mutations.
                let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
                let page_size = usize::try_from(page_size).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "cannot determine page size")
                })?;
                // c_ulong varies with the target ABI; convert losslessly before validating.
                #[allow(
                    clippy::useless_conversion,
                    reason = "smem_start is 32-bit on ARM32 and 64-bit on the host"
                )]
                let physical = u64::from(fix.smem_start);
                let offset = physical_offset(physical, len, page_size)?;
                let mem = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_SYNC | libc::O_CLOEXEC)
                    .open("/dev/mem")
                    .map_err(|error| {
                        io::Error::new(error.kind(), format!("open /dev/mem: {error}"))
                    })?;
                let mapping = map(&mem, len, offset)?;
                tracing::warn!(
                    physical,
                    len,
                    "fb0 mmap returned ENODEV; using /dev/mem framebuffer fallback"
                );
                Ok(mapping)
            },
        )
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// /dev/mem may be Device memory on ARM: libc memset/memcpy can issue
    /// unaligned accesses that fault even though the mapping itself is aligned.
    pub fn clear(&mut self) {
        let ptr = self.ptr.cast::<u32>();
        for word in 0..self.len / 4 {
            // SAFETY: mmap returns a page-aligned base; each word is in bounds.
            unsafe {
                ptr.add(word).write_volatile(0);
            }
        }
        for byte in self.len / 4 * 4..self.len {
            // SAFETY: remaining bytes are in bounds; byte stores need no alignment.
            unsafe {
                self.ptr.cast::<u8>().add(byte).write_volatile(0);
            }
        }
    }

    #[inline]
    pub fn write_word(&mut self, byte_offset: usize, value: u32) -> bool {
        if !byte_offset.is_multiple_of(4)
            || byte_offset.checked_add(4).is_none_or(|end| end > self.len)
        {
            return false;
        }
        // SAFETY: base is page-aligned and offset/range were checked above.
        // Volatile prevents compiler widening or replacing stores with memcpy.
        unsafe {
            self.ptr
                .cast::<u32>()
                .add(byte_offset / 4)
                .write_volatile(value);
        }
        true
    }
}

impl Drop for FbMapping {
    fn drop(&mut self) {
        // SAFETY: this owner holds exactly the successful mapping returned by map().
        unsafe {
            libc::munmap(self.ptr, self.len);
        }
    }
}

fn map(file: &File, len: usize, offset: libc::off64_t) -> io::Result<FbMapping> {
    // SAFETY: the fd is live and the caller validated its advertised range.
    // Explicit mmap64 avoids truncating physical addresses on ARM32 glibc.
    let ptr = unsafe {
        libc::mmap64(
            std::ptr::null_mut(),
            len,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            offset,
        )
    };
    if ptr == libc::MAP_FAILED {
        Err(io::Error::last_os_error())
    } else {
        Ok(FbMapping { ptr, len })
    }
}

fn physical_offset(physical: u64, len: usize, page_size: usize) -> io::Result<libc::off64_t> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid framebuffer physical range (missing, unaligned, or overflowing)",
        )
    };
    if physical == 0
        || len == 0
        || !page_size.is_power_of_two()
        || !physical.is_multiple_of(page_size as u64)
    {
        return Err(invalid());
    }
    let offset = i64::try_from(physical).map_err(|_| invalid())?;
    let length = i64::try_from(len).map_err(|_| invalid())?;
    offset.checked_add(length).ok_or_else(invalid)?;
    Ok(offset)
}

/// Keep eligibility and privileged access lazy: working kernels and unrelated
/// failures must never open /dev/mem or mask their original errno.
fn map_or_fallback<T>(
    native: io::Result<T>,
    eligible: impl FnOnce() -> bool,
    fallback: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    match native {
        Ok(mapping) => Ok(mapping),
        Err(error) if error.raw_os_error() == Some(libc::ENODEV) && eligible() => fallback()
            .map_err(|fallback| {
                io::Error::new(
                    fallback.kind(),
                    format!("mmap /dev/fb0: {error}; framebuffer fallback failed: {fallback}"),
                )
            }),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_mapping_is_shared_and_owned_without_device_access() -> io::Result<()> {
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::FileExt;
        // SAFETY: name is NUL-terminated and no pointer is retained by memfd_create.
        let fd = unsafe { libc::memfd_create(c"fb-mapping-test".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: memfd_create returned a fresh fd; File becomes its sole owner.
        let file = unsafe { File::from_raw_fd(fd) };
        file.set_len(4096)?;
        let mut mapping = FbMapping::open(
            &file,
            &FbFixScreeninfo {
                smem_len: 4096,
                ..Default::default()
            },
        )?;
        assert_eq!(mapping.len(), 4096);
        assert!(mapping.write_word(0, u32::from_ne_bytes([42, 1, 2, 3])));
        assert!(!mapping.write_word(1, 0));
        assert!(!mapping.write_word(4096, 0));
        assert!(!mapping.write_word(usize::MAX, 0));
        let mut bytes = [0; 4];
        file.read_exact_at(&mut bytes, 0)?;
        assert_eq!(bytes, [42, 1, 2, 3]);
        mapping.clear();
        file.read_exact_at(&mut bytes, 0)?;
        assert_eq!(bytes, [0; 4]);
        drop(mapping);
        file.read_exact_at(&mut bytes, 0)?;
        assert_eq!(bytes, [0; 4]);
        Ok(())
    }

    #[test]
    fn native_success_never_checks_or_opens_fallback() {
        let result = map_or_fallback(
            Ok(42),
            || unreachable!("native mmap worked"),
            || unreachable!("native mmap worked"),
        );
        assert_eq!(result.ok(), Some(42));
    }

    #[test]
    fn only_enodev_can_attempt_fallback() {
        for errno in [libc::EACCES, libc::EINVAL, libc::ENOMEM, libc::EBADF] {
            let result: io::Result<()> = map_or_fallback(
                Err(io::Error::from_raw_os_error(errno)),
                || unreachable!("not ENODEV"),
                || unreachable!("not ENODEV"),
            );
            assert_eq!(
                result.err().and_then(|error| error.raw_os_error()),
                Some(errno)
            );
        }
        let result = map_or_fallback(
            Err(io::Error::from_raw_os_error(libc::ENODEV)),
            || true,
            || Ok(7),
        );
        assert_eq!(result.ok(), Some(7));
    }

    #[test]
    fn disabled_or_non_framebuffer_preserves_original_error() {
        let result: io::Result<()> = map_or_fallback(
            Err(io::Error::from_raw_os_error(libc::ENODEV)),
            || false,
            || unreachable!("fallback is not eligible"),
        );
        assert_eq!(
            result.err().and_then(|error| error.raw_os_error()),
            Some(libc::ENODEV)
        );
    }

    #[test]
    fn fallback_failure_reports_both_causes() {
        let result: io::Result<()> = map_or_fallback(
            Err(io::Error::from_raw_os_error(libc::ENODEV)),
            || true,
            || Err(io::Error::from_raw_os_error(libc::EACCES)),
        );
        assert!(result
            .err()
            .is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied
                && error.to_string().contains("framebuffer fallback failed")));
    }

    #[test]
    fn validates_driver_range_without_truncating_arm32_physical_offsets() {
        assert_eq!(
            physical_offset(0x2200_1000, 8192, 4096).ok(),
            Some(0x2200_1000)
        );
        assert_eq!(
            physical_offset(0xE200_1000, 8192, 4096).ok(),
            Some(0xE200_1000)
        );
        for (physical, len, page) in [
            (0, 4096, 4096),
            (0x2200_1000, 0, 4096),
            (0x2200_1001, 4096, 4096),
            (0x2200_1000, 4096, 0),
            (u64::MAX, 4096, 1),
            (i64::MAX as u64, 4096, 1),
        ] {
            assert!(physical_offset(physical, len, page).is_err());
        }
    }
}
