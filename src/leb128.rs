use crate::error::{Error, Result};
use crate::{ensure, parse_err};

/// The maximum length of a leb128-encoded 32-bit integer
pub const MAX_LEB128_LEN_32: usize = 5;
pub const MAX_LEB128_LEN_64: usize = 10;

/// Returns the encoded size of an unsigned leb128-encoded integer.
#[inline]
fn size_u32(x: u32) -> usize {
    let bits = x.max(1).ilog2() + 1;
    ((9 * bits + 64) / 64) as usize
}

#[inline]
pub fn write_u32(buf: &mut [u8], mut x: u32) -> Result<usize> {
    ensure!(
        size_u32(x) <= buf.len(),
        Error::Parse("The number being read is too large for the provided buffer.".into())
    );

    for (i, curr_byte) in buf.iter_mut().enumerate().take(MAX_LEB128_LEN_32) {
        let byte = (x & 0x7F) as u8;
        x >>= 7;

        let more = (x != 0) as u8;
        *curr_byte = byte | (more << 7);

        if more == 0 {
            return Ok(i + 1);
        }
    }

    Err(Error::Parse(
        "The number being encoded exceeds the maximum representable length.".into(),
    ))
}

#[inline]
pub fn read_u32(buf: &[u8]) -> Result<(u32, usize)> {
    let mut x = 0u32;
    let mut s: usize = 0;

    for (i, &b) in buf.iter().enumerate() {
        ensure!(
            i < MAX_LEB128_LEN_32,
            Error::Parse("The number being decoded exceeds the maximum length of a leb128-encoded 32-bit integer.".into())
        );

        if b < 0x80 {
            if i == MAX_LEB128_LEN_32 - 1 && b > 0x0F {
                parse_err!("integer too large");
            }

            return Ok((x | (b as u32) << s, i + 1));
        }

        x |= ((b & 0x7f) as u32) << s;
        s += 7
    }

    parse_err!(
        "The number being decoded exceeds the maximum length of a leb128-encoded 32-bit integer."
    )
}

#[inline]
pub fn write_i32(buf: &mut [u8], mut x: i32) -> Result<usize> {
    let mut i = 0;
    loop {
        ensure!(
            i < buf.len() && i < MAX_LEB128_LEN_32,
            Error::Parse("buffer too small for signed LEB128 i32".into())
        );
        let mut byte = (x & 0x7F) as u8;
        x >>= 7;
        let done = (x == 0 && byte & 0x40 == 0) || (x == -1 && byte & 0x40 != 0);
        if !done {
            byte |= 0x80;
        }
        buf[i] = byte;
        i += 1;
        if done {
            return Ok(i);
        }
    }
}

#[inline]
pub fn read_i32(buf: &[u8]) -> Result<(i32, usize)> {
    let mut result: i32 = 0;
    let mut shift: u32 = 0;

    for (i, &byte) in buf.iter().enumerate() {
        ensure!(
            i < MAX_LEB128_LEN_32,
            Error::Parse("The number being decoded exceeds the maximum length of a signed leb128-encoded 32-bit integer.".into())
        );

        result |= ((byte & 0x7F) as i32) << shift;
        shift += 7;

        if byte & 0x80 == 0 {
            if i == MAX_LEB128_LEN_32 - 1 {
                let sign_bit = (byte >> 3) & 1;
                let expected_upper = if sign_bit == 1 { 0x7 } else { 0x0 };
                if (byte >> 4) & 0x7 != expected_upper {
                    parse_err!("integer too large");
                }
            }
            // Sign-extend if the sign bit (bit 6) of the last byte is set
            if shift < 32 && (byte & 0x40) != 0 {
                result |= !0i32 << shift;
            }
            return Ok((result, i + 1));
        }
    }

    parse_err!("Unterminated signed leb128-encoded 32-bit integer.")
}

#[inline]
pub fn read_u64(buf: &[u8]) -> Result<(u64, usize)> {
    let mut x = 0u64;
    let mut s: usize = 0;

    for (i, &b) in buf.iter().enumerate() {
        ensure!(
            i < MAX_LEB128_LEN_64,
            Error::Parse("The number being decoded exceeds the maximum length of a leb128-encoded 64-bit integer.".into())
        );

        if b < 0x80 {
            if i == MAX_LEB128_LEN_64 - 1 && b > 0x01 {
                parse_err!("integer too large");
            }

            return Ok((x | (b as u64) << s, i + 1));
        }

        x |= ((b & 0x7f) as u64) << s;
        s += 7
    }

    parse_err!(
        "The number being decoded exceeds the maximum length of a leb128-encoded 64-bit integer."
    )
}

#[inline]
pub fn read_i64(buf: &[u8]) -> Result<(i64, usize)> {
    let mut result: i64 = 0;
    let mut shift: u32 = 0;

    for (i, &byte) in buf.iter().enumerate() {
        ensure!(
            i < MAX_LEB128_LEN_64,
            Error::Parse("The number being decoded exceeds the maximum length of a signed leb128-encoded 64-bit integer.".into())
        );

        result |= ((byte & 0x7F) as i64) << shift;
        shift += 7;

        if byte & 0x80 == 0 {
            if i == MAX_LEB128_LEN_64 - 1 {
                let sign_bit = byte & 1;
                let expected_upper = if sign_bit == 1 { 0x7F } else { 0x0 };
                if (byte >> 1) != (expected_upper >> 1) {
                    parse_err!("integer too large");
                }
            }
            if shift < 64 && (byte & 0x40) != 0 {
                result |= !0i64 << shift;
            }
            return Ok((result, i + 1));
        }
    }

    parse_err!("Unterminated signed leb128-encoded 64-bit integer.")
}

#[cfg(all(test, not(any(feature = "core-tests", feature = "component-tests"))))]
mod tests {
    use super::*;

    fn test_i32_roundtrip(x: i32) -> Result<()> {
        let mut buf = [0u8; MAX_LEB128_LEN_32];
        let n = write_i32(&mut buf, x)?;
        let (y, m) = read_i32(&buf[0..n])?;
        assert_eq!(x, y, "Expected {}, got: {}", x, y);
        assert_eq!(n, m, "For {}, got {}, want: {}", x, m, n);
        Ok(())
    }

    fn test_u32_roundtrip(x: u32) -> Result<()> {
        let mut buf = [0u8; MAX_LEB128_LEN_32];
        let n = write_u32(&mut buf, x)?;
        let (y, m) = read_u32(&buf[0..n])?;
        assert_eq!(x, y, "Expected {}, got: {}", x, y);
        assert_eq!(n, m, "For {}, got {}, want: {}", x, m, n);
        Ok(())
    }

    #[test]
    fn signed_leb128_known_values() -> Result<()> {
        // Standard signed LEB128 test vectors
        assert_eq!(read_i32(&[0x00])?, (0, 1));
        assert_eq!(read_i32(&[0x01])?, (1, 1));
        assert_eq!(read_i32(&[0x07])?, (7, 1));
        assert_eq!(read_i32(&[0x7F])?, (-1, 1));
        assert_eq!(read_i32(&[0x7C])?, (-4, 1));
        assert_eq!(read_i32(&[0x80, 0x01])?, (128, 2));
        assert_eq!(read_i32(&[0xFF, 0x7E])?, (-129, 2));
        assert_eq!(read_i32(&[0xC0, 0xBB, 0x78])?, (-123456, 3));
        Ok(())
    }

    #[test]
    fn signed_leb128_roundtrip() -> Result<()> {
        for x in [
            i32::MIN,
            i32::MIN + 1,
            -1,
            0,
            1,
            2,
            10,
            63,
            64,
            127,
            128,
            255,
            256,
            i32::MAX,
        ] {
            test_i32_roundtrip(x)?;
        }
        Ok(())
    }

    #[test]
    fn unsigned_leb128_roundtrip() -> Result<()> {
        for x in [0u32, 1, 2, 10, 63, 64, 127, 128, 255, 256, u32::MAX] {
            test_u32_roundtrip(x)?;
        }
        Ok(())
    }

    #[test]
    fn non_canonical_zero() -> Result<()> {
        let buf = [0x80, 0x80, 0x80, 0];
        let (x, n) = read_u32(&buf)?;
        assert_eq!(x, 0);
        assert_eq!(n, 4);
        Ok(())
    }

    #[test]
    fn overflow() -> Result<()> {
        let overflow_cases = [
            vec![0xFF, 0xFF, 0xFF, 0xFF, 0xFF],
            vec![0x80, 0x80, 0x80, 0x80, 0x80, 0x80],
        ];
        for buffer in overflow_cases {
            assert!(
                read_u32(&buffer).is_err(),
                "Expected overflow for buffer: {:?}",
                buffer
            );
        }
        Ok(())
    }
}
