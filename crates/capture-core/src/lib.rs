pub mod desktop;
mod encoder;
pub mod recording;
pub mod startup;
pub mod store;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
impl Region {
    pub fn validate(self, desktop: Self) -> Result<Self> {
        if self.width < 2 || self.height < 2 || self.width > 16384 || self.height > 16384 {
            bail!("Capture dimensions must be between 2 and 16384 physical pixels");
        }
        if self.x < desktop.x
            || self.y < desktop.y
            || i64::from(self.x) + i64::from(self.width)
                > i64::from(desktop.x) + i64::from(desktop.width)
            || i64::from(self.y) + i64::from(self.height)
                > i64::from(desktop.y) + i64::from(desktop.height)
        {
            bail!("Capture region is outside the current desktop");
        }
        // Bound allocations even on extremely large virtual desktops.
        if u64::from(self.width) * u64::from(self.height) > 40_000_000 {
            bail!("Capture exceeds the 40 megapixel limit");
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_negative_monitor_origins_and_overflow() {
        let desktop = Region {
            x: -1920,
            y: 0,
            width: 3840,
            height: 1080,
        };
        assert!(Region {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080
        }
        .validate(desktop)
        .is_ok());
        assert!(Region {
            x: i32::MAX,
            y: 0,
            width: u32::MAX,
            height: 2
        }
        .validate(desktop)
        .is_err());
        assert!(Region {
            x: -1921,
            y: 0,
            width: 2,
            height: 2
        }
        .validate(desktop)
        .is_err());
    }
}
