//! Self-localization: find our violet viewfinder frame inside a
//! fullscreen stream frame.
//!
//! Wayland never tells a client its own position, so at record time we
//! scan live PipeWire frames for the marker frame painted around the
//! viewfinder (solid violet bars at the widget bounds). The bars are
//! isolated as a connected component and validated against the expected
//! device-pixel size, so a wrong or occluded hit fails loudly instead of
//! mis-cropping the GIF.

use std::collections::HashSet;

use peek_core::{PeekError, PhysicalRect};

/// Marker color: Ubuntu's violet accent `(116, 96, 212)` — the same
/// tone as the quality slider — opaque. Cairo renders solid fills at
/// exact byte values, so matching is a tight band around the sampled
/// value: ordinary UI blues, pure blue, and dark grays all fall outside
/// it. Other violet UI (the slider knob, accent text elsewhere) can fall
/// inside the band too, which is why detection works on connected
/// components and size validation, never on a bounding box over every
/// match: only the frame-shaped component wins.
const MARK_R: (u8, u8) = (106, 126);
const MARK_G: (u8, u8) = (86, 106);
const MARK_B: (u8, u8) = (202, 222);

fn is_mark(p: &[u8]) -> bool {
    (MARK_R.0..=MARK_R.1).contains(&p[0])
        && (MARK_G.0..=MARK_G.1).contains(&p[1])
        && (MARK_B.0..=MARK_B.1).contains(&p[2])
}

/// Size mismatch tolerance in device pixels (covers fractional-scale
/// rounding of the marker thickness).
const DIM_TOL: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    /// Marker bounds in stream device pixels.
    pub rect: PhysicalRect,
}

/// One 4-connected blob of marker-colored pixels.
struct Component {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    count: u64,
}

impl Component {
    fn size(&self) -> (u32, u32) {
        (
            (self.x1 - self.x0 + 1) as u32,
            (self.y1 - self.y0 + 1) as u32,
        )
    }
}

/// Find the viewfinder marker in one RGBA stream frame. `expected_w` /
/// `expected_h` are the widget bounds converted to device pixels; only a
/// component within [`DIM_TOL`] of that size can win.
pub fn find_frame_rgba(
    px: &[u8],
    width: u32,
    height: u32,
    expected_w: u32,
    expected_h: u32,
) -> Result<PhysicalRect, PeekError> {
    let expected_len = width as usize * height as usize * 4;
    if px.len() != expected_len {
        return Err(PeekError::InvalidFrame(format!(
            "buffer is {} bytes, expected {expected_len}",
            px.len()
        )));
    }
    let w = width as usize;
    let (chunks, _) = px.as_chunks::<4>();

    // Flood-fill 4-connected components over matching pixels only, so
    // the cost scales with the marker, not with the frame.
    let mut pending: HashSet<u32> = chunks
        .iter()
        .enumerate()
        .filter(|(_, p)| is_mark(*p))
        .map(|(i, _)| i as u32)
        .collect();

    let mut candidates: Vec<Component> = Vec::new();
    while let Some(&seed) = pending.iter().next() {
        pending.remove(&seed);
        let mut stack = vec![seed];
        let mut c = Component {
            x0: i64::MAX,
            y0: i64::MAX,
            x1: i64::MIN,
            y1: i64::MIN,
            count: 0,
        };
        while let Some(i) = stack.pop() {
            let (x, y) = ((i as usize % w) as i64, (i as usize / w) as i64);
            c.x0 = c.x0.min(x);
            c.y0 = c.y0.min(y);
            c.x1 = c.x1.max(x);
            c.y1 = c.y1.max(y);
            c.count += 1;
            let mut visit = |n: u32, stack: &mut Vec<u32>| {
                if pending.remove(&n) {
                    stack.push(n);
                }
            };
            if x > 0 {
                visit(i - 1, &mut stack);
            }
            if x + 1 < w as i64 {
                visit(i + 1, &mut stack);
            }
            if i as usize >= w {
                visit(i - w as u32, &mut stack);
            }
            if i as usize + w < chunks.len() {
                visit(i + w as u32, &mut stack);
            }
        }
        candidates.push(c);
    }

    let fits = |c: &Component| {
        let (cw, ch) = c.size();
        cw.abs_diff(expected_w) <= DIM_TOL && ch.abs_diff(expected_h) <= DIM_TOL
    };
    let Some(best) = candidates
        .iter()
        .filter(|c| fits(c))
        .max_by_key(|c| c.count)
    else {
        if candidates.is_empty() {
            return Err(PeekError::Capture(
                "viewfinder marker not found in stream frames (wrong monitor picked?)".into(),
            ));
        }
        let seen = candidates
            .iter()
            .map(|c| {
                let (cw, ch) = c.size();
                format!("{cw}x{ch}")
            })
            .take(6)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(PeekError::Capture(format!(
            "no violet component matches the viewfinder size {expected_w}x{expected_h} (seen: {seen}) — wrong monitor, frame occluded or partly offscreen, or window moved mid-detect"
        )));
    };
    let (bw, bh) = best.size();
    // A complete frame carries at least a single-pixel perimeter worth
    // of marker pixels; less means bars are covered by another window.
    if best.count < (bw + bh) as u64 {
        return Err(PeekError::Capture(
            "viewfinder frame partially occluded; move other windows clear".into(),
        ));
    }
    PhysicalRect::new(best.x0 as i32, best.y0 as i32, bw, bh)
        .map_err(|_| PeekError::Capture("detected rect degenerate".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARK: [u8; 3] = [116, 96, 212];

    /// Opaque dark background, nothing like the marker color.
    fn canvas(w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for p in px.as_chunks_mut::<4>().0 {
            *p = [30, 40, 50, 255];
        }
        px
    }

    fn put(px: &mut [u8], w: usize, x: usize, y: usize, c: [u8; 3]) {
        let i = (y * w + x) * 4;
        px[i..i + 3].copy_from_slice(&c);
        px[i + 3] = 255;
    }

    /// Hollow rectangle with outer bounds `(x, y, w, h)` and thickness `t`.
    fn frame(px: &mut [u8], cw: usize, x: usize, y: usize, w: usize, h: usize, t: usize) {
        for yy in y..y + h {
            for xx in x..x + w {
                if xx < x + t || xx >= x + w - t || yy < y + t || yy >= y + h - t {
                    put(px, cw, xx, yy, MARK);
                }
            }
        }
    }

    #[test]
    fn finds_frame_at_exact_bounds() {
        let (w, h) = (200usize, 140usize);
        let mut px = canvas(w, h);
        frame(&mut px, w, 20, 30, 100, 60, 3);
        let rect = find_frame_rgba(&px, 200, 140, 100, 60).unwrap();
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (20, 30, 100, 60));
    }

    #[test]
    fn ignores_violet_widgets_elsewhere() {
        // The real failure mode: the header's accent-violet slider is a
        // separate small blob and must not join the marker's bbox.
        let (w, h) = (200usize, 140usize);
        let mut px = canvas(w, h);
        frame(&mut px, w, 20, 30, 100, 60, 3);
        for yy in 0..4 {
            for xx in 150..168 {
                put(&mut px, w, xx, yy, MARK);
            }
        }
        let rect = find_frame_rgba(&px, 200, 140, 100, 60).unwrap();
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (20, 30, 100, 60));
    }

    #[test]
    fn picks_matching_component_among_many() {
        // Several unrelated violet blobs, only one frame-sized.
        let (w, h) = (400usize, 300usize);
        let mut px = canvas(w, h);
        for (bx, by) in [(10, 10), (300, 20), (20, 250)] {
            for yy in by..by + 8 {
                for xx in bx..bx + 20 {
                    put(&mut px, w, xx, yy, MARK);
                }
            }
        }
        frame(&mut px, w, 40, 100, 120, 80, 3);
        let rect = find_frame_rgba(&px, 400, 300, 120, 80).unwrap();
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (40, 100, 120, 80)
        );
    }

    #[test]
    fn rejects_wrong_expected_size() {
        let (w, h) = (200usize, 140usize);
        let mut px = canvas(w, h);
        frame(&mut px, w, 20, 30, 100, 60, 3);
        assert!(find_frame_rgba(&px, 200, 140, 400, 300).is_err());
    }

    #[test]
    fn rejects_blank_desktop() {
        let px = vec![30u8; 200 * 140 * 4];
        assert!(find_frame_rgba(&px, 200, 140, 100, 60).is_err());
    }

    #[test]
    fn rejects_occluded_frame() {
        // Keep only the four 3x3 corners: no component is frame-sized,
        // so the occluded frame is rejected.
        let (w, h) = (200usize, 140usize);
        let mut px = canvas(w, h);
        for (cx, cy) in [(20, 30), (117, 30), (20, 87), (117, 87)] {
            for dy in 0..3 {
                for dx in 0..3 {
                    put(&mut px, w, cx + dx, cy + dy, MARK);
                }
            }
        }
        assert!(find_frame_rgba(&px, 200, 140, 100, 60).is_err());
    }

    #[test]
    fn tolerates_rendering_wobble() {
        // Cairo quantization may land a channel a tick off pure violet;
        // near-violet still matches.
        let (w, h) = (32usize, 32usize);
        let mut px = canvas(w, h);
        for y in 0..h {
            for x in 0..w {
                if x < 3 || y < 3 || x >= w - 3 || y >= h - 3 {
                    put(&mut px, w, x, y, [113, 93, 209]);
                }
            }
        }
        let rect = find_frame_rgba(&px, 32, 32, 32, 32).unwrap();
        assert_eq!((rect.x, rect.y, rect.width, rect.height), (0, 0, 32, 32));
    }

    #[test]
    fn ignores_pure_blue() {
        // Pure blue (0, 0, 255) must not match the violet band.
        let (w, h) = (32usize, 32usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                px[i + 2] = 255;
                px[i + 3] = 255;
            }
        }
        assert!(find_frame_rgba(&px, 32, 32, 32, 32).is_err());
    }

    #[test]
    fn ignores_ordinary_dark_blues() {
        // Editor-style background (30, 30, 46): dark, not violet.
        let (w, h) = (32usize, 32usize);
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                px[i] = 30;
                px[i + 1] = 30;
                px[i + 2] = 46;
                px[i + 3] = 255;
            }
        }
        assert!(find_frame_rgba(&px, 32, 32, 32, 32).is_err());
    }
}
