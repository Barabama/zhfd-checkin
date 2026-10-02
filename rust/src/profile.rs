use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn from_dimensions(width: u32, height: u32) -> Self {
        if width >= height {
            Self::Landscape
        } else {
            Self::Portrait
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: &'static str,
    pub orientation: Orientation,
    pub width: u32,
    pub height: u32,
    pub density_dpi: u32,
    pub button_center_ratio: (f32, f32),
    pub button_box_ratio: (f32, f32),
    pub service_icon_center_ratio: (f32, f32),
    pub calibrated: bool,
}

// The successful historical LDPlayer run was 900x1600 at the device
// reported 320 dpi. The required 240 dpi presets remain non-live until each
// preset passes an in-window state/interaction validation run. The portrait
// 900x1600@240 profile passed the first in-window dry-run on 2026-09-26 and
// is the only 240 dpi profile currently enabled for guarded live verification.
// Its fallback was measured on 2026-09-25 at (449,778) with a 232 px circle.
pub const PROFILES: &[Profile] = &[
    Profile {
        id: "landscape_1600x900_d240",
        orientation: Orientation::Landscape,
        width: 1600,
        height: 900,
        density_dpi: 240,
        // 2026-09-25 gray-state measurement: center≈(800,779), 232 px.
        button_center_ratio: (0.5000, 0.8656),
        button_box_ratio: (0.1450, 0.2578),
        service_icon_center_ratio: (0.14, 0.59),
        calibrated: false,
    },
    Profile {
        id: "portrait_900x1600_d240",
        orientation: Orientation::Portrait,
        width: 900,
        height: 1600,
        density_dpi: 240,
        button_center_ratio: (0.4994, 0.4863),
        button_box_ratio: (0.2578, 0.1450),
        service_icon_center_ratio: (0.1417, 0.5919),
        // 2026-09-26 in-window dry-run: locating -> ready -> ready,
        // dry_run_ready=true, clicked=false. Enable only this profile for
        // the first guarded live verification.
        calibrated: true,
    },
    Profile {
        id: "landscape_1280x720_d240",
        orientation: Orientation::Landscape,
        width: 1280,
        height: 720,
        density_dpi: 240,
        // 2026-09-25 gray-state measurement after one WebView scroll:
        // center≈(640,410), 256 px.
        button_center_ratio: (0.5000, 0.5694),
        button_box_ratio: (0.2000, 0.3556),
        service_icon_center_ratio: (0.14, 0.59),
        calibrated: false,
    },
    Profile {
        id: "portrait_720x1280_d240",
        orientation: Orientation::Portrait,
        width: 720,
        height: 1280,
        density_dpi: 240,
        // 2026-09-25 gray-state measurement: center≈(360,853), 256 px.
        button_center_ratio: (0.5000, 0.6664),
        button_box_ratio: (0.3556, 0.2000),
        service_icon_center_ratio: (0.1417, 0.5919),
        calibrated: false,
    },
    Profile {
        id: "landscape_1920x1080_d280",
        orientation: Orientation::Landscape,
        width: 1920,
        height: 1080,
        density_dpi: 280,
        // 2026-09-25 gray-state measurement: center≈(960,908), 308 px.
        button_center_ratio: (0.5000, 0.8407),
        button_box_ratio: (0.1604, 0.2852),
        service_icon_center_ratio: (0.14, 0.59),
        calibrated: false,
    },
    Profile {
        id: "portrait_1080x1920_d280",
        orientation: Orientation::Portrait,
        width: 1080,
        height: 1920,
        density_dpi: 280,
        // 2026-09-25 gray-state measurement: center≈(540,907), 308 px.
        button_center_ratio: (0.5000, 0.4724),
        button_box_ratio: (0.2852, 0.1604),
        service_icon_center_ratio: (0.1417, 0.5919),
        calibrated: false,
    },
    Profile {
        id: "landscape_960x540_d160",
        orientation: Orientation::Landscape,
        width: 960,
        height: 540,
        density_dpi: 160,
        // 2026-09-25 gray-state measurement: center≈(480,352), 178 px.
        button_center_ratio: (0.5000, 0.6519),
        button_box_ratio: (0.1854, 0.3296),
        service_icon_center_ratio: (0.14, 0.59),
        calibrated: false,
    },
    Profile {
        id: "portrait_900x1600_d320",
        orientation: Orientation::Portrait,
        width: 900,
        height: 1600,
        density_dpi: 320,
        button_center_ratio: (0.50, 0.673),
        button_box_ratio: (0.36, 0.20),
        service_icon_center_ratio: (0.1417, 0.5919),
        calibrated: true,
    },
    Profile {
        id: "portrait_540x960_d160",
        orientation: Orientation::Portrait,
        width: 540,
        height: 960,
        density_dpi: 160,
        // 2026-09-25 gray-state measurement: center≈(270,525), 178 px.
        button_center_ratio: (0.5000, 0.5469),
        button_box_ratio: (0.3296, 0.1854),
        service_icon_center_ratio: (0.1417, 0.5919),
        calibrated: false,
    },
];

pub fn find_profile(width: u32, height: u32, density_dpi: u32) -> Option<&'static Profile> {
    let orientation = Orientation::from_dimensions(width, height);
    PROFILES.iter().find(|p| {
        p.orientation == orientation
            && p.width == width
            && p.height == height
            && p.density_dpi == density_dpi
    })
}

pub fn find_profile_by_id(id: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRED: &[(u32, u32, u32, &str, Orientation)] = &[
        (
            1600,
            900,
            240,
            "landscape_1600x900_d240",
            Orientation::Landscape,
        ),
        (
            900,
            1600,
            240,
            "portrait_900x1600_d240",
            Orientation::Portrait,
        ),
        (
            1280,
            720,
            240,
            "landscape_1280x720_d240",
            Orientation::Landscape,
        ),
        (
            720,
            1280,
            240,
            "portrait_720x1280_d240",
            Orientation::Portrait,
        ),
        (
            1920,
            1080,
            280,
            "landscape_1920x1080_d280",
            Orientation::Landscape,
        ),
        (
            1080,
            1920,
            280,
            "portrait_1080x1920_d280",
            Orientation::Portrait,
        ),
        (
            960,
            540,
            160,
            "landscape_960x540_d160",
            Orientation::Landscape,
        ),
        (
            540,
            960,
            160,
            "portrait_540x960_d160",
            Orientation::Portrait,
        ),
    ];

    #[test]
    fn all_eight_advertised_profiles_match_exactly() {
        for (width, height, dpi, id, orientation) in REQUIRED {
            let profile = find_profile(*width, *height, *dpi).expect("required profile missing");
            assert_eq!(profile.id, *id);
            assert_eq!(profile.orientation, *orientation);
            if *id == "portrait_900x1600_d240" {
                assert!(profile.calibrated);
            } else {
                assert!(
                    !profile.calibrated,
                    "unverified profile must not be click-enabled"
                );
            }
        }
    }

    #[test]
    fn rejects_near_match_wrong_dpi_and_swapped_dimensions() {
        assert!(find_profile(900, 1600, 241).is_none());
        assert!(find_profile(901, 1600, 240).is_none());
        assert!(find_profile(1600, 900, 280).is_none());
    }

    #[test]
    fn measured_240_profiles_have_profile_specific_geometry() {
        let expected = [
            ("landscape_1600x900_d240", (0.50, 0.8656), (0.1450, 0.2578)),
            ("portrait_900x1600_d240", (0.4994, 0.4863), (0.2578, 0.1450)),
            ("landscape_1280x720_d240", (0.50, 0.5694), (0.20, 0.3556)),
            ("portrait_720x1280_d240", (0.50, 0.6664), (0.3556, 0.20)),
            ("landscape_1920x1080_d280", (0.50, 0.8407), (0.1604, 0.2852)),
            ("portrait_1080x1920_d280", (0.50, 0.4724), (0.2852, 0.1604)),
            ("landscape_960x540_d160", (0.50, 0.6519), (0.1854, 0.3296)),
            ("portrait_540x960_d160", (0.50, 0.5469), (0.3296, 0.1854)),
        ];
        for (id, center, region) in expected {
            let profile = find_profile_by_id(id).expect("measured profile missing");
            assert_eq!(profile.button_center_ratio, center);
            assert_eq!(profile.button_box_ratio, region);
            assert_eq!(profile.calibrated, id == "portrait_900x1600_d240");
        }
    }

    #[test]
    fn existing_320_dpi_ldplayer_profile_is_separate_and_calibrated() {
        let profile = find_profile(900, 1600, 320).unwrap();
        assert_eq!(profile.id, "portrait_900x1600_d320");
        assert!(profile.calibrated);
    }
}
