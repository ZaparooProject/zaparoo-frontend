// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Vblank-latch scanout protocol v4, implemented clean-room from the
// machine-readable spec `latch-protocol.json` shipped with the Menu
// fork's latch RTL (commands 0x57-0x5B on the menu core's io_uio
// path). Transport is deliberately out of scope here: whether the
// frontend drives the uio bridge directly under a Main-granted lease
// or hands words to a relay, the packing, CRC, and parsing are
// identical. Golden vectors from the spec pin the implementation.

#![allow(
    dead_code,
    reason = "protocol layer lands ahead of the transport; consumed by the latch presenter next"
)]

pub const PROTOCOL_VERSION: u16 = 4;

pub const CMD_SET: u16 = 0x57;
pub const CMD_GET: u16 = 0x58;
pub const CMD_CAPS: u16 = 0x59;
pub const CMD_DIAGNOSTICS: u16 = 0x5A;
pub const CMD_RECEIPT: u16 = 0x5B;

// Zaparoo fork extension: the Menu fork's latch and scanout-slots
// module carry native 1080p RGB565 limits (upstream MagiK ships
// 1366x768/2736). The caps probe still validates the loaded core's
// actual limits at runtime; these cap our side of the min().
pub const LIMIT_MAX_WIDTH: u16 = 1920;
pub const LIMIT_MAX_HEIGHT: u16 = 1080;
pub const LIMIT_MAX_STRIDE_BYTES: u16 = 3840;

/// CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, no reflection, no
/// final xor) over the header words (command, protocol version,
/// non-CRC word count) followed by the payload words, each word fed
/// high byte first.
pub fn crc16(command: u16, payload: &[u16]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    let mut feed = |word: u16| {
        for byte in [(word >> 8) as u8, (word & 0xFF) as u8] {
            crc ^= u16::from(byte) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x1021
                } else {
                    crc << 1
                };
            }
        }
    };
    feed(command);
    feed(PROTOCOL_VERSION);
    feed(payload.len() as u16);
    for &word in payload {
        feed(word);
    }
    crc
}

/// A v4 set (post) command: publish a frame's base address, geometry,
/// and destination rectangle. Word semantics per `set_words_v4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetCommand {
    pub mode: u16,
    pub base: u32,
    pub width: u16,
    pub height: u16,
    pub destination_left: u16,
    pub destination_right: u16,
    pub destination_top: u16,
    pub destination_bottom: u16,
    pub stride: u16,
    pub sequence: u16,
}

impl SetCommand {
    /// The 12 words written after the 0x57 command byte: 11 payload
    /// words then the CRC.
    pub fn words(&self) -> [u16; 12] {
        let payload = [
            self.mode,
            (self.base & 0xFFFF) as u16,
            (self.base >> 16) as u16,
            self.width,
            self.height,
            self.destination_left,
            self.destination_right,
            self.destination_top,
            self.destination_bottom,
            self.stride,
            self.sequence,
        ];
        let crc = crc16(CMD_SET, &payload);
        [
            payload[0],
            payload[1],
            payload[2],
            payload[3],
            payload[4],
            payload[5],
            payload[6],
            payload[7],
            payload[8],
            payload[9],
            payload[10],
            crc,
        ]
    }
}

/// Parsed v4 status response (command 0x58): 15 payload words + CRC,
/// per `status_words_v4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub active_sequence: u16,
    pub pending_sequence: u16,
    pub flags: u16,
    pub flip_count: u16,
    pub accepted_count: u16,
    pub active_base: u32,
    pub active_width: u16,
    pub active_height: u16,
    pub active_stride: u16,
    pub reject_count: u16,
    pub active_route_epoch: u16,
    pub active_transaction: u16,
    pub pending_transaction: u16,
    pub accepted_transaction: u16,
}

/// Status flag bits per `status_flags`.
impl Status {
    pub fn active_enabled(&self) -> bool {
        self.flags & (1 << 0) != 0
    }
    pub fn pending_enabled(&self) -> bool {
        self.flags & (1 << 1) != 0
    }
    pub fn pending(&self) -> bool {
        self.flags & (1 << 2) != 0
    }
    pub fn magik_ownership(&self) -> bool {
        self.flags & (1 << 3) != 0
    }
    pub fn reject_reason(&self) -> u16 {
        (self.flags >> 4) & 0xF
    }
}

/// Parsed v4 post receipt (command 0x5B): 10 payload words + CRC, per
/// `receipt_words_v4`. The authoritative answer to "did my post land".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    pub attempted_transaction: u16,
    pub attempted_sequence: u16,
    pub disposition: u16,
    pub accepted_transaction: u16,
    pub accepted_sequence: u16,
    pub pending_transaction: u16,
    pub pending_sequence: u16,
    pub active_transaction: u16,
    pub active_sequence: u16,
    pub reject_reason: u16,
}

pub const DISPOSITION_NONE: u16 = 0;
pub const DISPOSITION_ACCEPTED: u16 = 1;
pub const DISPOSITION_REJECTED: u16 = 2;

/// Rejection reason names per `rejection_reasons`, for log lines.
pub fn reject_reason_name(reason: u16) -> &'static str {
    match reason {
        0 => "none",
        1 => "missing_word",
        2 => "duplicate_word",
        3 => "out_of_order",
        4 => "post_close",
        5 => "bad_crc",
        6 => "invalid_mode",
        7 => "invalid_base",
        8 => "invalid_geometry",
        9 => "invalid_stride",
        10 => "invalid_bounds",
        11 => "address_wrap",
        12 => "restarted",
        13 => "shifted_word",
        14 => "pending_busy",
        _ => "reserved",
    }
}

pub fn parse_receipt(words: &[u16]) -> Result<Receipt, ParseError> {
    if words.len() != 11 {
        return Err(ParseError::BadLength);
    }
    let (payload, crc) = words.split_at(10);
    let expected = crc16(CMD_RECEIPT, payload);
    if crc[0] != expected {
        return Err(ParseError::BadCrc {
            expected,
            got: crc[0],
        });
    }
    Ok(Receipt {
        attempted_transaction: payload[0],
        attempted_sequence: payload[1],
        disposition: payload[2],
        accepted_transaction: payload[3],
        accepted_sequence: payload[4],
        pending_transaction: payload[5],
        pending_sequence: payload[6],
        active_transaction: payload[7],
        active_sequence: payload[8],
        reject_reason: payload[9],
    })
}

/// Parsed v4 capability response (command 0x59): 5 payload words +
/// CRC. Used as the latch-presence probe: a stock menu core answers
/// with unrelated garbage that fails the CRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub version: u16,
    pub flags: u16,
    pub max_width: u16,
    pub max_height: u16,
    pub max_stride_bytes: u16,
}

pub fn parse_caps(words: &[u16]) -> Result<Caps, ParseError> {
    if words.len() != 6 {
        return Err(ParseError::BadLength);
    }
    let (payload, crc) = words.split_at(5);
    let expected = crc16(CMD_CAPS, payload);
    if crc[0] != expected {
        return Err(ParseError::BadCrc {
            expected,
            got: crc[0],
        });
    }
    Ok(Caps {
        version: payload[0],
        flags: payload[1],
        max_width: payload[2],
        max_height: payload[3],
        max_stride_bytes: payload[4],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    BadLength,
    BadCrc { expected: u16, got: u16 },
}

pub fn parse_status(words: &[u16]) -> Result<Status, ParseError> {
    if words.len() != 16 {
        return Err(ParseError::BadLength);
    }
    let (payload, crc) = words.split_at(15);
    let expected = crc16(CMD_GET, payload);
    if crc[0] != expected {
        return Err(ParseError::BadCrc {
            expected,
            got: crc[0],
        });
    }
    Ok(Status {
        active_sequence: payload[0],
        pending_sequence: payload[1],
        flags: payload[2],
        flip_count: payload[3],
        accepted_count: payload[4],
        active_base: u32::from(payload[5]) | (u32::from(payload[6]) << 16),
        active_width: payload[7],
        active_height: payload[8],
        active_stride: payload[9],
        reject_count: payload[10],
        active_route_epoch: payload[11],
        active_transaction: payload[12],
        pending_transaction: payload[13],
        accepted_transaction: payload[14],
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::decimal_bitwise_operands,
        reason = "tests should fail-fast and quote the spec's decimal golden values verbatim"
    )]

    use super::*;

    // Golden vectors from latch-protocol.json.

    #[test]
    fn caps_golden_crc() {
        assert_eq!(crc16(CMD_CAPS, &[4, 511, 1366, 768, 2736]), 15780);
    }

    #[test]
    fn set_golden_crc_and_packing() {
        let cmd = SetCommand {
            mode: 32788,
            base: (8830_u32 << 16) | 36864,
            width: 960,
            height: 540,
            destination_left: 0,
            destination_right: 959,
            destination_top: 0,
            destination_bottom: 539,
            stride: 1920,
            sequence: 43,
        };
        let words = cmd.words();
        assert_eq!(
            &words[..11],
            &[32788, 36864, 8830, 960, 540, 0, 959, 0, 539, 1920, 43]
        );
        assert_eq!(words[11], 22261);
    }

    #[test]
    fn status_golden_parses() {
        let words = [
            42, 43, 15, 3, 4, 36864, 8830, 960, 540, 1920, 7, 9, 100, 101, 101, 37242,
        ];
        let status = parse_status(&words).expect("golden status must parse");
        assert_eq!(status.active_sequence, 42);
        assert_eq!(status.pending_sequence, 43);
        assert_eq!(status.flip_count, 3);
        assert_eq!(status.active_base, (8830 << 16) | 36864);
        assert_eq!(status.active_stride, 1920);
        assert_eq!(status.reject_count, 7);
        // flags = 15: active_enabled, pending_enabled, pending, ownership.
        assert!(status.active_enabled());
        assert!(status.pending_enabled());
        assert!(status.pending());
        assert!(status.magik_ownership());
        assert_eq!(status.reject_reason(), 0);
    }

    #[test]
    fn diagnostics_golden_crc() {
        assert_eq!(crc16(CMD_DIAGNOSTICS, &[7, 1, 11, 0, 88, 0]), 61309);
    }

    #[test]
    fn receipt_golden_crc() {
        assert_eq!(
            crc16(CMD_RECEIPT, &[101, 43, 1, 101, 43, 101, 43, 100, 42, 0]),
            22657
        );
    }

    #[test]
    fn receipt_golden_parses() {
        let words = [101, 43, 1, 101, 43, 101, 43, 100, 42, 0, 22657];
        let receipt = parse_receipt(&words).expect("golden receipt must parse");
        assert_eq!(receipt.attempted_sequence, 43);
        assert_eq!(receipt.disposition, DISPOSITION_ACCEPTED);
        assert_eq!(receipt.accepted_sequence, 43);
        assert_eq!(receipt.reject_reason, 0);
    }

    #[test]
    fn caps_golden_parses() {
        let words = [4, 511, 1366, 768, 2736, 15780];
        let caps = parse_caps(&words).expect("golden caps must parse");
        assert_eq!(caps.version, 4);
        assert_eq!(caps.flags, 511);
        assert_eq!(caps.max_width, 1366);
        assert_eq!(caps.max_height, 768);
        assert_eq!(caps.max_stride_bytes, 2736);
        // A stock core's garbage answer must not parse.
        let garbage = [0_u16; 6];
        assert!(parse_caps(&garbage).is_err());
    }

    #[test]
    fn corrupted_status_is_rejected() {
        let mut words = [
            42, 43, 15, 3, 4, 36864, 8830, 960, 540, 1920, 7, 9, 100, 101, 101, 37242,
        ];
        words[3] = 99;
        assert!(matches!(
            parse_status(&words),
            Err(ParseError::BadCrc { .. })
        ));
        assert_eq!(parse_status(&words[..10]), Err(ParseError::BadLength));
    }
}
