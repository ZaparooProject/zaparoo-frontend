// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Reads the pi-top-style SMBus fuel gauge that `Main_MiSTer`'s own
// `battery.cpp` already polls for its System Information page: I2C address
// 0x0B, register 0x0D for capacity (0-100). Not present on stock `MiSTer`
// hardware -- this only produces a reading when the same optional battery
// HAT `Main_MiSTer` supports is actually attached, mirroring its quiet
// "no battery found" fallback. See `Main_MiSTer/battery.cpp` and
// `Main_MiSTer/smbus.cpp` for the reference this follows.
//
// Deliberately does not carry `getBattery`'s static "give up forever after
// the first miss" cache: that exists there to skip syscalls on a page
// redrawn every 2s while open, but `SystemStatus` only polls this once per
// `LOCAL_PROBE_INTERVAL`, so a fresh open+probe each tick is cheap and lets
// a HAT plugged in after launch be picked up without a restart. Each probe
// also closes its own fd rather than leaking one across calls the way
// `getBattery` does, since this runs for the life of the process rather
// than only while a menu page is open.

#[cfg(zaparoo_runtime = "mister")]
use std::ffi::CString;
#[cfg(zaparoo_runtime = "mister")]
use std::os::unix::io::RawFd;
#[cfg(zaparoo_runtime = "mister")]
use std::time::Duration;

#[cfg(zaparoo_runtime = "mister")]
const I2C_SLAVE: libc::c_ulong = 0x0703;
#[cfg(zaparoo_runtime = "mister")]
const I2C_SMBUS: libc::c_ulong = 0x0720;
#[cfg(zaparoo_runtime = "mister")]
const I2C_SMBUS_READ: u8 = 1;
#[cfg(zaparoo_runtime = "mister")]
const I2C_SMBUS_WRITE: u8 = 0;
#[cfg(zaparoo_runtime = "mister")]
const I2C_SMBUS_QUICK: u32 = 0;
#[cfg(zaparoo_runtime = "mister")]
const I2C_SMBUS_WORD_DATA: u32 = 3;
#[cfg(zaparoo_runtime = "mister")]
const BATTERY_I2C_ADDRESS: libc::c_int = 0x0B;
#[cfg(zaparoo_runtime = "mister")]
const CAPACITY_REGISTER: u8 = 0x0D;
#[cfg(zaparoo_runtime = "mister")]
const MAX_TRIES: u32 = 20;
#[cfg(zaparoo_runtime = "mister")]
const RETRY_SLEEP: Duration = Duration::from_micros(500);

#[cfg(zaparoo_runtime = "mister")]
#[repr(C)]
union SmbusData {
    byte: u8,
    word: u16,
    block: [u8; 34],
}

#[cfg(zaparoo_runtime = "mister")]
#[repr(C)]
struct SmbusIoctlData {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut SmbusData,
}

#[cfg(zaparoo_runtime = "mister")]
fn smbus_access(fd: RawFd, read_write: u8, command: u8, size: u32, data: *mut SmbusData) -> bool {
    let mut args = SmbusIoctlData {
        read_write,
        command,
        size,
        data,
    };
    // SAFETY: `fd` is a valid, open i2c-dev file descriptor for the
    // duration of this call, and `args` is a live stack value laid out to
    // match the kernel's `i2c_smbus_ioctl_data` (see linux/i2c-dev.h).
    unsafe { libc::ioctl(fd, I2C_SMBUS, std::ptr::addr_of_mut!(args)) == 0 }
}

#[cfg(zaparoo_runtime = "mister")]
fn read_word(fd: RawFd, register: u8) -> Option<u16> {
    let mut data = SmbusData { word: 0 };
    if !smbus_access(
        fd,
        I2C_SMBUS_READ,
        register,
        I2C_SMBUS_WORD_DATA,
        std::ptr::addr_of_mut!(data),
    ) {
        return None;
    }
    // SAFETY: a successful I2C_SMBUS_WORD_DATA transfer populates `word`.
    Some(unsafe { data.word })
}

#[cfg(zaparoo_runtime = "mister")]
fn open_bus(bus: u32) -> Option<RawFd> {
    let path = CString::new(format!("/dev/i2c-{bus}")).ok()?;
    // SAFETY: `path` is a valid, live, NUL-terminated C string for the
    // duration of this call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    (fd >= 0).then_some(fd)
}

#[cfg(zaparoo_runtime = "mister")]
fn select_battery_device(fd: RawFd) -> bool {
    // SAFETY: `fd` is a valid, open file descriptor.
    let selected = unsafe { libc::ioctl(fd, I2C_SLAVE, BATTERY_I2C_ADDRESS) == 0 };
    selected
        && smbus_access(
            fd,
            I2C_SMBUS_WRITE,
            0,
            I2C_SMBUS_QUICK,
            std::ptr::null_mut(),
        )
}

#[cfg(zaparoo_runtime = "mister")]
fn close_bus(fd: RawFd) {
    // SAFETY: `fd` was returned by a successful `open_bus` above and is not
    // used again after this call.
    unsafe {
        libc::close(fd);
    }
}

/// Register 0x0D on the pi-top-style fuel gauge, reinterpreted the same way
/// `Main_MiSTer`'s `getReg` narrows an `SMBus` word read into its signed
/// `short capacity` field: a failed transfer or a value outside the sensor's
/// documented 0-100 range is not a real reading.
#[cfg(any(zaparoo_runtime = "mister", test))]
fn validate_capacity(raw_word: u16) -> Option<u8> {
    let signed = raw_word as i16;
    (0..=100).contains(&signed).then_some(signed as u8)
}

/// Battery capacity percentage (0-100), or `None` when no fuel gauge
/// answers on the bus -- the common case on stock `MiSTer` hardware.
/// Probes `/dev/i2c-0` through `/dev/i2c-2` in turn, matching
/// `Main_MiSTer`'s own default bus scan order.
#[cfg(zaparoo_runtime = "mister")]
pub fn read_capacity_percent() -> Option<u8> {
    for bus in 0..=2u32 {
        let Some(fd) = open_bus(bus) else {
            continue;
        };
        if !select_battery_device(fd) {
            close_bus(fd);
            continue;
        }

        let mut percent = None;
        for _ in 0..MAX_TRIES {
            if let Some(value) = read_word(fd, CAPACITY_REGISTER).and_then(validate_capacity) {
                percent = Some(value);
                break;
            }
            std::thread::sleep(RETRY_SLEEP);
        }

        close_bus(fd);
        return percent;
    }
    None
}

#[cfg(not(zaparoo_runtime = "mister"))]
pub fn read_capacity_percent() -> Option<u8> {
    None
}

#[cfg(test)]
mod tests {
    use super::validate_capacity;

    #[test]
    fn accepts_values_in_range() {
        assert_eq!(validate_capacity(0), Some(0));
        assert_eq!(validate_capacity(50), Some(50));
        assert_eq!(validate_capacity(100), Some(100));
    }

    #[test]
    fn rejects_values_above_range() {
        assert_eq!(validate_capacity(101), None);
        assert_eq!(validate_capacity(255), None);
    }

    #[test]
    fn rejects_negative_sentinel_and_errno_style_words() {
        // 0xFFFF is -1 as a signed 16-bit word -- the initial "no reading
        // yet" sentinel `getReg` retries on.
        assert_eq!(validate_capacity(0xFFFF), None);
        // A small negative errno (e.g. -5 for EIO) narrowed the same way.
        assert_eq!(validate_capacity(0xFFFB), None);
    }
}
