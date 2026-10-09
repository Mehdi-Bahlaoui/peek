//! Software RGBA crop. The portal gives whole-monitor streams, so the
//! selector rect is cut out of each frame on the CPU. Plain row copies:
//! exact, stride-safe, and fast enough next to gifski.

use peek_core::{PeekError, PhysicalRect};

/// Copy `rect` out of a tightly-packed RGBA buffer.
pub fn crop_rgba(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    rect: &PhysicalRect,
) -> Result<Vec<u8>, PeekError> {
    let expected = src_width as usize * src_height as usize * 4;
    if src.len() != expected {
        return Err(PeekError::InvalidFrame(format!(
            "buffer is {} bytes, expected {expected}",
            src.len()
        )));
    }
    let (x, y, w, h) = (
        rect.x as i64,
        rect.y as i64,
        rect.width as i64,
        rect.height as i64,
    );
    if x < 0 || y < 0 || w <= 0 || h <= 0 || x + w > src_width as i64 || y + h > src_height as i64 {
        return Err(PeekError::InvalidFrame(format!(
            "crop {}x{}+{}+{} outside {}x{} frame",
            rect.width, rect.height, rect.x, rect.y, src_width, src_height
        )));
    }
    let stride = src_width as usize * 4;
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for row in 0..h as usize {
        let start = ((y as usize + row) * stride) + x as usize * 4;
        out.extend_from_slice(&src[start..start + w as usize * 4]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_4x4() -> Vec<u8> {
        // Pixel (x, y) = [x, y, 0, 255].
        let mut v = Vec::with_capacity(4 * 4 * 4);
        for y in 0u8..4 {
            for x in 0u8..4 {
                v.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        v
    }

    #[test]
    fn cuts_center_block() {
        let src = frame_4x4();
        // Below core's MIN_SIZE on purpose: production crops are always
        // >= 16px (enforced by LogicalRect), but the copy logic itself
        // is size-agnostic, so the test uses a struct literal.
        let rect = PhysicalRect {
            x: 1,
            y: 1,
            width: 2,
            height: 2,
        };
        let out = crop_rgba(&src, 4, 4, &rect).unwrap();
        assert_eq!(
            out,
            vec![1, 1, 0, 255, 2, 1, 0, 255, 1, 2, 0, 255, 2, 2, 0, 255]
        );
    }

    #[test]
    fn rejects_out_of_bounds() {
        let src = frame_4x4();
        let rect = PhysicalRect {
            x: 3,
            y: 3,
            width: 2,
            height: 2,
        };
        assert!(crop_rgba(&src, 4, 4, &rect).is_err());
    }

    #[test]
    fn rejects_bad_buffer() {
        let rect = PhysicalRect {
            x: 0,
            y: 0,
            width: 2,
            height: 2,
        };
        assert!(crop_rgba(&[0u8; 10], 4, 4, &rect).is_err());
    }
}
