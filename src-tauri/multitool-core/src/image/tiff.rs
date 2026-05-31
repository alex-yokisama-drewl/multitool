//! Multi-frame TIFF detection.
//!
//! Tools whose contract is "preserve the source format" (`image_crop`,
//! `image_resize`) can't honor that contract for multi-page TIFFs: the
//! `image` crate's decoder silently hands back the first frame and the
//! orchestrator would write a single-frame TIFF in its place — quiet data
//! loss. [`tiff_frame_count`] runs before decode so those tools can refuse
//! up-front with `UnsupportedFormat`.

/// Count top-level images (IFDs) in a **classic** TIFF byte stream.
///
/// Returns `Some(n)` for a parseable classic TIFF, `None` for anything else
/// (non-TIFF magic, BigTIFF, or a truncated/malformed chain) — in which case
/// the caller falls through to the normal decoder rather than guessing.
///
/// Walks the IFD linked list: header → first-IFD offset → each IFD is a
/// `u16` entry count, `count × 12` bytes of entries, then a `u32` offset to
/// the next IFD (`0` terminates). The walk is bounded so a corrupt/looping
/// chain can't spin. Any out-of-bounds read aborts to `None`.
pub fn tiff_frame_count(bytes: &[u8]) -> Option<u32> {
    let little_endian = match bytes.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let read_u16 = |off: usize| -> Option<u16> {
        let s = bytes.get(off..off + 2)?;
        Some(if little_endian {
            u16::from_le_bytes([s[0], s[1]])
        } else {
            u16::from_be_bytes([s[0], s[1]])
        })
    };
    let read_u32 = |off: usize| -> Option<u32> {
        let s = bytes.get(off..off + 4)?;
        Some(if little_endian {
            u32::from_le_bytes([s[0], s[1], s[2], s[3]])
        } else {
            u32::from_be_bytes([s[0], s[1], s[2], s[3]])
        })
    };

    // Classic TIFF magic is 42; BigTIFF (43) uses 8-byte offsets we don't
    // parse — bail to None and let the decoder handle it.
    if read_u16(2)? != 42 {
        return None;
    }

    let mut ifd_offset = read_u32(4)? as usize;
    let mut count: u32 = 0;
    for _ in 0..1024 {
        if ifd_offset == 0 {
            break;
        }
        let entries = read_u16(ifd_offset)? as usize;
        count = count.saturating_add(1);
        let next_off_pos = ifd_offset.checked_add(2 + entries.checked_mul(12)?)?;
        ifd_offset = read_u32(next_off_pos)? as usize;
    }
    Some(count)
}

/// Minimal classic little-endian TIFF header with `frames` empty IFDs
/// chained together. Enough for [`tiff_frame_count`] to walk the chain; not
/// a decodable image (rejection happens before decode). Shared between this
/// module's own tests and the `image_crop` tests that prove rejection
/// propagates end-to-end.
#[cfg(test)]
pub(crate) fn multi_ifd_tiff(frames: usize) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(b"II"); // little-endian
    b.extend_from_slice(&42u16.to_le_bytes()); // classic magic
                                               // First IFD starts right after the 8-byte header.
    b.extend_from_slice(&8u32.to_le_bytes());
    for i in 0..frames {
        // Each IFD: count=0 entries, then a 4-byte next-IFD offset.
        b.extend_from_slice(&0u16.to_le_bytes());
        let is_last = i + 1 == frames;
        let next = if is_last {
            0u32
        } else {
            // Next IFD sits immediately after this 6-byte IFD.
            u32::try_from(b.len() + 4).unwrap()
        };
        b.extend_from_slice(&next.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn images_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/images/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read images fixture {name}: {path:?}"))
    }

    fn format_fixture(name: &str) -> Vec<u8> {
        let path = PathBuf::from(format!("tests/fixtures/image_format/{name}"));
        fs::read(&path).unwrap_or_else(|_| panic!("read image_format fixture {name}: {path:?}"))
    }

    #[test]
    fn counts_chained_ifds() {
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(1)), Some(1));
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(2)), Some(2));
        assert_eq!(tiff_frame_count(&multi_ifd_tiff(4)), Some(4));
    }

    #[test]
    fn is_none_for_non_tiff() {
        assert_eq!(tiff_frame_count(&images_fixture("red.png")), None);
        assert_eq!(tiff_frame_count(&images_fixture("blue.jpg")), None);
        assert_eq!(tiff_frame_count(b""), None);
        assert_eq!(tiff_frame_count(b"II"), None); // truncated
    }

    #[test]
    fn single_frame_real_tiff_counts_one() {
        assert_eq!(tiff_frame_count(&format_fixture("tiny.tif")), Some(1));
    }
}
