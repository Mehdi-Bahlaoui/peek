//! Recording rectangle types.
//!
//! Two coordinate spaces exist on Wayland, and mixing them up is the
//! classic capture bug, so they are distinct types:
//!
//! * [`LogicalRect`] — what the user drags in the selector UI
//!   (desktop logical pixels).
//! * [`PhysicalRect`] — what the PipeWire stream delivers (the real pixels).
//!   (device pixels = logical × monitor scale).

use serde::{Deserialize, Serialize};

use crate::peek_error::PeekError;

/// Smallest recordable edge, in logical pixels.
pub const MIN_SIZE: u32 = 16;
/// Largest recordable edge, in logical pixels (8K cap).
pub const MAX_DIMENSION: u32 = 7680;

/// Corner / edge handle dragged during resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandle {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

/// User-space rectangle from the selector overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Stream-space rectangle after monitor-scale mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

fn check_dims(width: u32, height: u32) -> Result<(), PeekError> {
    if width < MIN_SIZE || height < MIN_SIZE {
        return Err(PeekError::InvalidRect(format!(
            "size {width}x{height} below minimum {min}x{min}",
            min = MIN_SIZE
        )));
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PeekError::InvalidRect(format!(
            "size {width}x{height} above maximum {MAX_DIMENSION}"
        )));
    }
    Ok(())
}

impl LogicalRect {
    /// Validated constructor. Never panics; all failures are [`PeekError`].
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self, PeekError> {
        check_dims(width, height)?;
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }

    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && (px as i64) < self.x as i64 + self.width as i64
            && (py as i64) < self.y as i64 + self.height as i64
    }

    /// Translate without changing size (drag-move).
    pub fn moved_by(&self, dx: i32, dy: i32) -> Self {
        Self {
            x: self.x.saturating_add(dx),
            y: self.y.saturating_add(dy),
            width: self.width,
            height: self.height,
        }
    }

    /// Resize via a handle drag. `dx`/`dy` are cursor deltas in logical
    /// pixels. The opposite edge stays anchored; size clamps at [`MIN_SIZE`].
    pub fn resized(&self, handle: ResizeHandle, dx: i32, dy: i32) -> Self {
        let (mut x, mut y, mut w, mut h) = (self.x, self.y, self.width as i64, self.height as i64);
        let min = MIN_SIZE as i64;

        match handle {
            ResizeHandle::E | ResizeHandle::NE | ResizeHandle::SE => {
                w = (w + dx as i64).max(min);
            }
            ResizeHandle::W | ResizeHandle::NW | ResizeHandle::SW => {
                let new_w = (w - dx as i64).max(min);
                x += (w - new_w) as i32;
                w = new_w;
            }
            _ => {}
        }
        match handle {
            ResizeHandle::S | ResizeHandle::SE | ResizeHandle::SW => {
                h = (h + dy as i64).max(min);
            }
            ResizeHandle::N | ResizeHandle::NE | ResizeHandle::NW => {
                let new_h = (h - dy as i64).max(min);
                y += (h - new_h) as i32;
                h = new_h;
            }
            _ => {}
        }

        Self {
            x,
            y,
            width: w as u32,
            height: h as u32,
        }
    }

    /// Clamp inside monitor bounds (intersection, preserving top-left).
    /// Returns the clamped rect, or an error if there is no overlap.
    pub fn clamp_to(&self, bounds: &LogicalRect) -> Result<Self, PeekError> {
        let x0 = self.x.max(bounds.x);
        let y0 = self.y.max(bounds.y);
        let self_x1 = self.x as i64 + self.width as i64;
        let self_y1 = self.y as i64 + self.height as i64;
        let bounds_x1 = bounds.x as i64 + bounds.width as i64;
        let bounds_y1 = bounds.y as i64 + bounds.height as i64;
        let x1 = self_x1.min(bounds_x1);
        let y1 = self_y1.min(bounds_y1);
        let w = (x1 - x0 as i64).max(0) as u32;
        let h = (y1 - y0 as i64).max(0) as u32;
        Self::new(x0, y0, w, h)
            .map_err(|_| PeekError::InvalidRect("rect lies fully outside monitor bounds".into()))
    }

    /// Map to stream pixels. `monitor_scale` is e.g. 1.0, 1.5, 2.0.
    pub fn to_physical(&self, monitor_scale: f64) -> Result<PhysicalRect, PeekError> {
        if !(1.0..=4.0).contains(&monitor_scale) {
            return Err(PeekError::InvalidMonitorScale(monitor_scale));
        }
        let w = ((self.width as f64 * monitor_scale).round() as u32).max(2);
        let h = ((self.height as f64 * monitor_scale).round() as u32).max(2);
        let x = ((self.x as f64 * monitor_scale).round()) as i32;
        let y = ((self.y as f64 * monitor_scale).round()) as i32;
        PhysicalRect::new(x, y, w, h)
    }
}

impl PhysicalRect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Result<Self, PeekError> {
        check_dims(width, height)?;
        Ok(Self {
            x,
            y,
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_degenerate_sizes() {
        assert!(LogicalRect::new(0, 0, 0, 100).is_err());
        assert!(LogicalRect::new(0, 0, 100, 15).is_err());
        assert!(LogicalRect::new(0, 0, 8000, 100).is_err());
        assert!(LogicalRect::new(0, 0, 16, 16).is_ok());
    }

    #[test]
    fn move_preserves_size() {
        let r = LogicalRect::new(10, 20, 300, 200).unwrap();
        let m = r.moved_by(-15, 40);
        assert_eq!((m.x, m.y, m.width, m.height), (-5, 60, 300, 200));
    }

    #[test]
    fn east_grows_west_anchors_opposite_edge() {
        let r = LogicalRect::new(100, 100, 300, 200).unwrap();
        let e = r.resized(ResizeHandle::E, 50, 0);
        assert_eq!((e.x, e.width), (100, 350));
        let w = r.resized(ResizeHandle::W, 50, 0);
        assert_eq!((w.x, w.width), (150, 250));
    }

    #[test]
    fn north_moves_top_edge() {
        let r = LogicalRect::new(100, 100, 300, 200).unwrap();
        let n = r.resized(ResizeHandle::N, 0, 20);
        assert_eq!((n.y, n.height), (120, 180));
        let se = r.resized(ResizeHandle::SE, 10, 10);
        assert_eq!((se.width, se.height), (310, 210));
    }

    #[test]
    fn resize_never_below_minimum() {
        let r = LogicalRect::new(0, 0, 20, 20).unwrap();
        let shrunk = r.resized(ResizeHandle::SE, -1000, -1000);
        assert_eq!((shrunk.width, shrunk.height), (MIN_SIZE, MIN_SIZE));
    }

    #[test]
    fn clamp_cuts_overflow() {
        let bounds = LogicalRect::new(0, 0, 1920, 1080).unwrap();
        let r = LogicalRect::new(1800, 1000, 300, 200).unwrap();
        let c = r.clamp_to(&bounds).unwrap();
        assert_eq!((c.x, c.y, c.width, c.height), (1800, 1000, 120, 80));
        let outside = LogicalRect::new(2000, 2000, 100, 100).unwrap();
        assert!(outside.clamp_to(&bounds).is_err());
    }

    #[test]
    fn physical_mapping_applies_scale() {
        // Your reference clip: 694x494 logical at 1.0 stays identical.
        let r = LogicalRect::new(0, 0, 694, 494).unwrap();
        let p = r.to_physical(1.0).unwrap();
        assert_eq!((p.width, p.height), (694, 494));
        // At 200% the stream delivers 4x the pixels.
        let p2 = r.to_physical(2.0).unwrap();
        assert_eq!((p2.width, p2.height), (1388, 988));
        // Fractional scale rounds.
        let odd = LogicalRect::new(0, 0, 101, 101).unwrap();
        assert_eq!(odd.to_physical(1.5).unwrap().width, 152);
        assert!(r.to_physical(0.5).is_err());
    }
}
