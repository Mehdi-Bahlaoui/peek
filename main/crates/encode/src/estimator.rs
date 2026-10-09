//! Pre-record size warning. GIF has no motion compensation, so size
//! grows linearly with pixels × fps × seconds. Calibrated against a real
//! capture: 694x494 @30fps UI content measured ~1.66MB per 23s, which the
//! model below predicts almost exactly.

use peek_core::Fps;

/// Measured bytes per pixel per frame for typical desktop UI content.
const BYTES_PER_PX_FRAME: f64 = 0.007;
const LARGE_BYTES: u64 = 8_000_000;
const HUGE_BYTES: u64 = 25_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeLevel {
    Ok,
    Large,
    Huge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Estimate {
    pub bytes: u64,
    pub level: SizeLevel,
}

pub fn estimate(pixels: u64, fps: Fps, quality: u8, seconds: u64) -> Estimate {
    let bytes = (pixels as f64
        * f64::from(fps.value())
        * seconds as f64
        * BYTES_PER_PX_FRAME
        * f64::from(quality)
        / 80.0) as u64;
    let level = if bytes > HUGE_BYTES {
        SizeLevel::Huge
    } else if bytes > LARGE_BYTES {
        SizeLevel::Large
    } else {
        SizeLevel::Ok
    };
    Estimate { bytes, level }
}

/// Human-readable warning, or `None` when the size is fine.
pub fn warn(est: &Estimate) -> Option<String> {
    let mb = est.bytes as f64 / 1_000_000.0;
    match est.level {
        SizeLevel::Ok => None,
        SizeLevel::Large => Some(format!(
            "Large file expected (~{mb:.1} MB). Lower fps, quality, or record a smaller area."
        )),
        SizeLevel::Huge => Some(format!(
            "Huge file expected (~{mb:.1} MB). 30fps GIF at this size is not practical — shrink the rect or drop to 10fps."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_clip_is_ok() {
        // Your clip: 694x494, 30fps, 23s, measured 1.69MB.
        let est = estimate(694 * 494, Fps::Fps30, 80, 23);
        assert!(est.bytes > 1_200_000 && est.bytes < 2_200_000);
        assert_eq!(est.level, SizeLevel::Ok);
        assert_eq!(warn(&est), None);
    }

    #[test]
    fn fullscreen_30fps_warns() {
        let large = estimate(1920 * 1080, Fps::Fps30, 80, 20);
        assert_eq!(large.level, SizeLevel::Large);
        assert!(warn(&large).unwrap().contains("Large file"));

        let huge = estimate(1920 * 1080, Fps::Fps30, 100, 60);
        assert_eq!(huge.level, SizeLevel::Huge);
        assert!(warn(&huge).unwrap().contains("Huge file"));
    }

    #[test]
    fn quality_scales_estimate() {
        let hi = estimate(640 * 480, Fps::Fps10, 100, 10);
        let lo = estimate(640 * 480, Fps::Fps10, 50, 10);
        assert!(hi.bytes > lo.bytes);
    }
}
