// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Userspace client for the menu core's io_uio (SSPI) word channel,
// used to drive the vblank-latch scanout protocol while Main has
// granted the uio lease (it spawned us with --latch and gates its own
// FPGA writes until we exit).
//
// Interface derivation (clean-room, no GPL code):
// - Register block: Cyclone V HPS FPGA-manager, physical 0xFF706000
//   (Intel Cyclone V HPS TRM); the general-purpose handshake registers
//   gpo (+0x10, HPS-to-FPGA) and gpi (+0x14, FPGA-to-HPS).
// - Wire contract, from the Zaparoo Menu fork's sys_top.v (our own
//   tree): gpo[15:0] carries the word, gpo[17] is the strobe/clock,
//   gpo[20] selects the io_uio channel, and gpo[31] must stay high -
//   the core answers gpi with its identity magic while gpo[31] is low.
//   The core mirrors the strobe back on gpi[17] two clk_sys cycles
//   after registering each edge; the reply word is valid in gpi[15:0]
//   once the mirrored strobe has fallen.
// - Framing, from the latch bridge RTL: while the channel is selected
//   the first strobed word is the command id (low byte), every further
//   strobed word is data; deselecting resets the framing. Response
//   words are clocked out by strobing dummy zero words.

use std::fs::OpenOptions;
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt as _;

const FPGA_MANAGER_PHYS: libc::off_t = 0xFF70_6000;
const MAP_LEN: usize = 0x1000;
const GPO_OFFSET: usize = 0x10;
const GPI_OFFSET: usize = 0x14;

const DATA_MASK: u32 = 0xFFFF;
const STROBE: u32 = 1 << 17;
const UIO_SELECT: u32 = 1 << 20;
const CORE_COMM_FLAG: u32 = 0x8000_0000;

/// Bounded busy-wait iterations for one strobe acknowledgment. The
/// core mirrors the strobe within a few `clk_sys` cycles; even a slow
/// 50 MHz `clk_sys` answers in well under a microsecond, so hitting
/// this limit means the core is gone (FPGA reconfiguring, wrong RBF).
const ACK_SPIN_LIMIT: u32 = 2_000_000;

pub struct Uio {
    base: *mut u8,
    /// gpo bits outside the SSPI fields, as found at open. Restored
    /// around every transaction so we never clobber unrelated state.
    shadow: u32,
}

// SAFETY: the register pointer is only used from the render thread
// that owns the Uio; the type is moved, never shared.
unsafe impl Send for Uio {}

#[derive(Debug)]
pub enum UioError {
    Io(io::Error),
    /// gpi never mirrored a strobe edge; the core stopped answering.
    AckTimeout,
}

impl std::fmt::Display for UioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "uio: {e}"),
            Self::AckTimeout => write!(f, "uio: ack timeout (core not answering)"),
        }
    }
}

impl Uio {
    pub fn open() -> Result<Self, UioError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/mem")
            .map_err(UioError::Io)?;
        // SAFETY: mapping one page of the FPGA-manager register block;
        // MAP_SHARED so stores reach the device.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                MAP_LEN,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                FPGA_MANAGER_PHYS,
            )
        };
        if base == libc::MAP_FAILED {
            return Err(UioError::Io(io::Error::last_os_error()));
        }
        // `file` may drop: the mapping keeps the physical pages.
        let mut uio = Self {
            base: base.cast(),
            shadow: 0,
        };
        // Capture Main's parked gpo state (data/strobe/select cleared)
        // and make sure the comm flag stays up.
        uio.shadow = (uio.read_gpo() & !(DATA_MASK | STROBE | UIO_SELECT)) | CORE_COMM_FLAG;
        Ok(uio)
    }

    #[allow(
        clippy::cast_ptr_alignment,
        reason = "base is a page-aligned mmap and both register offsets are 4-byte aligned"
    )]
    fn reg(&self, offset: usize) -> *mut u32 {
        // SAFETY: offset is GPO_OFFSET or GPI_OFFSET, both within the
        // live MAP_LEN mapping owned by self.
        unsafe { self.base.add(offset).cast::<u32>() }
    }

    fn read_gpo(&self) -> u32 {
        // SAFETY: reg() points into the live device mapping.
        unsafe { std::ptr::read_volatile(self.reg(GPO_OFFSET)) }
    }

    fn write_gpo(&mut self, value: u32) {
        // SAFETY: as read_gpo; volatile store to the device register.
        unsafe { std::ptr::write_volatile(self.reg(GPO_OFFSET), value) };
    }

    fn read_gpi(&self) -> u32 {
        // SAFETY: as read_gpo.
        unsafe { std::ptr::read_volatile(self.reg(GPI_OFFSET)) }
    }

    /// One 16-bit word exchange on the selected channel: present the
    /// word, raise the strobe, wait for the mirrored strobe, drop it,
    /// wait for the mirror to fall, then read the reply.
    fn xfer(&mut self, select: u32, word: u16) -> Result<u16, UioError> {
        let level = self.shadow | select | u32::from(word);
        self.write_gpo(level);
        self.write_gpo(level | STROBE);
        self.wait_ack(true)?;
        self.write_gpo(level);
        self.wait_ack(false)?;
        Ok((self.read_gpi() & DATA_MASK) as u16)
    }

    fn wait_ack(&self, high: bool) -> Result<(), UioError> {
        for _ in 0..ACK_SPIN_LIMIT {
            let mirrored = self.read_gpi() & STROBE != 0;
            if mirrored == high {
                return Ok(());
            }
        }
        Err(UioError::AckTimeout)
    }

    /// Run one uio command transaction: select the channel, send the
    /// command word, exchange `words` in place (each entry is sent and
    /// replaced by the word clocked back), deselect. Deselecting also
    /// resets the bridge framing, so an error mid-transaction leaves
    /// the receiver clean for the next attempt.
    pub fn transact(&mut self, command: u16, words: &mut [u16]) -> Result<(), UioError> {
        let result = self.transact_inner(command, words);
        // Always deselect, also on the error path.
        self.write_gpo(self.shadow);
        result
    }

    fn transact_inner(&mut self, command: u16, words: &mut [u16]) -> Result<(), UioError> {
        self.write_gpo(self.shadow | UIO_SELECT);
        self.xfer(UIO_SELECT, command)?;
        for word in words {
            *word = self.xfer(UIO_SELECT, *word)?;
        }
        Ok(())
    }
}

impl Drop for Uio {
    fn drop(&mut self) {
        // Park the bus deselected with the comm flag up, then unmap.
        self.write_gpo(self.shadow);
        // SAFETY: base came from mmap(MAP_LEN) in open().
        unsafe { libc::munmap(self.base.cast(), MAP_LEN) };
    }
}
