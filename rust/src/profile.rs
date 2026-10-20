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

/// Resolution/DPI presets offered by the LDPlayer 14 UI (read from the
/// 2026-10-21 screenshots).
///
/// This is the *catalog* of presets, kept separate from the geometry table
/// below: every entry must have a matching [`Profile`], while [`PROFILES`] may
/// also retain legacy entries that the current UI no longer offers.
pub const LDPLAYER_PRESETS: &[(u32, u32, u32)] = &[
    // 平板(横屏)
    (1920, 1080, 280),
    (1600, 900, 240),
    (1280, 720, 240),
    (960, 540, 160),
    // 手机(竖屏)
    (1080, 1920, 480),
    (900, 1600, 320),
    (720, 1280, 320),
    (540, 960, 240),
];

// The successful historical LDPlayer run was 900x1600 at the device
// reported 320 dpi. Most presets remain non-live until each passes an
// in-window state/interaction validation run. The portrait 900x1600@240
// profile was the first in-window dry-run (2026-09-26) and is retained as a
// legacy entry for older installs, although the current LDPlayer UI offers
// 900x1600@320 instead. `portrait_900x1600_d320` is the profile used by the
// original 900x1600 production instances.
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
        // dry_run_ready=true, clicked=false. Retained as the historical
        // calibrated profile for older installs; it is not a current preset.
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
        id: "portrait_720x1280_d320",
        orientation: Orientation::Portrait,
        width: 720,
        height: 1280,
        density_dpi: 320,
        // Ratios were measured at 240 dpi before the LDPlayer preset changed
        // to 320 dpi. Placeholder only; live stays disabled until re-measured.
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
        id: "portrait_1080x1920_d480",
        orientation: Orientation::Portrait,
        width: 1080,
        height: 1920,
        density_dpi: 480,
        // Ratios were measured at 280 dpi before the LDPlayer preset changed
        // to 480 dpi. They are kept only as a dry-run placeholder; this profile
        // stays uncalibrated (no live click) until re-measured at 480 dpi.
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
        id: "portrait_540x960_d240",
        orientation: Orientation::Portrait,
        width: 540,
        height: 960,
        density_dpi: 240,
        // Ratios were measured at 160 dpi before the LDPlayer preset changed
        // to 240 dpi. Placeholder only; live stays disabled until re-measured.
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

/// Whether this geometry is one of the current LDPlayer UI presets, as opposed
/// to a legacy profile kept for older installs.
pub fn is_ldplayer_preset(width: u32, height: u32, density_dpi: u32) -> bool {
    LDPLAYER_PRESETS.contains(&(width, height, density_dpi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ldplayer_preset_has_a_matching_profile() {
        assert_eq!(LDPLAYER_PRESETS.len(), 8);
        for (width, height, dpi) in LDPLAYER_PRESETS {
            let profile = find_profile(*width, *height, *dpi)
                .unwrap_or_else(|| panic!("missing profile for {width}x{height}@{dpi}"));
            assert_eq!(
                profile.orientation,
                Orientation::from_dimensions(*width, *height)
            );
            // Only the current production profile is click-enabled among the
            // advertised presets. The changed-DPI placeholders must stay
            // dry-run-only until their geometry is re-measured.
            if profile.id == "portrait_900x1600_d320" {
                assert!(profile.calibrated);
            } else {
                assert!(
                    !profile.calibrated,
                    "unverified preset must not be click-enabled"
                );
            }
        }
    }

    #[test]
    fn legacy_900x1600_d240_profile_is_retained_but_not_a_preset() {
        let legacy = find_profile(900, 1600, 240).expect("legacy profile missing");
        assert_eq!(legacy.id, "portrait_900x1600_d240");
        assert!(
            legacy.calibrated,
            "historical calibrated profile is kept for old installs"
        );
        assert!(!LDPLAYER_PRESETS.contains(&(900, 1600, 240)));
        assert!(LDPLAYER_PRESETS.contains(&(900, 1600, 320)));
    }

    #[test]
    fn presets_and_profiles_are_unique() {
        for (index, (width, height, dpi)) in LDPLAYER_PRESETS.iter().enumerate() {
            assert!(
                !LDPLAYER_PRESETS[..index].contains(&(*width, *height, *dpi)),
                "duplicate preset {width}x{height}@{dpi}"
            );
        }
        for (index, profile) in PROFILES.iter().enumerate() {
            assert!(
                !PROFILES[..index].iter().any(|other| {
                    other.width == profile.width
                        && other.height == profile.height
                        && other.density_dpi == profile.density_dpi
                }),
                "duplicate geometry for {}",
                profile.id
            );
            assert!(
                !PROFILES[..index].iter().any(|other| other.id == profile.id),
                "duplicate id {}",
                profile.id
            );
        }
    }

    #[test]
    fn rejects_near_match_wrong_dpi_and_swapped_dimensions() {
        assert!(find_profile(900, 1600, 241).is_none());
        assert!(find_profile(901, 1600, 240).is_none());
        assert!(find_profile(1600, 900, 280).is_none());
        // The old portrait DPIs are no longer presets and must not match.
        assert!(find_profile(1080, 1920, 280).is_none());
        assert!(find_profile(720, 1280, 240).is_none());
        assert!(find_profile(540, 960, 160).is_none());
    }

    #[test]
    fn measured_profiles_have_profile_specific_geometry() {
        let expected = [
            ("landscape_1600x900_d240", (0.50, 0.8656), (0.1450, 0.2578)),
            ("portrait_900x1600_d240", (0.4994, 0.4863), (0.2578, 0.1450)),
            ("landscape_1280x720_d240", (0.50, 0.5694), (0.20, 0.3556)),
            ("portrait_720x1280_d320", (0.50, 0.6664), (0.3556, 0.20)),
            ("landscape_1920x1080_d280", (0.50, 0.8407), (0.1604, 0.2852)),
            ("portrait_1080x1920_d480", (0.50, 0.4724), (0.2852, 0.1604)),
            ("landscape_960x540_d160", (0.50, 0.6519), (0.1854, 0.3296)),
            ("portrait_540x960_d240", (0.50, 0.5469), (0.3296, 0.1854)),
        ];
        for (id, center, region) in expected {
            let profile = find_profile_by_id(id).expect("measured profile missing");
            assert_eq!(profile.button_center_ratio, center);
            assert_eq!(profile.button_box_ratio, region);
            assert_eq!(profile.calibrated, id == "portrait_900x1600_d240");
        }
    }

    #[test]
    fn current_320_dpi_preset_is_calibrated() {
        let profile = find_profile(900, 1600, 320).unwrap();
        assert_eq!(profile.id, "portrait_900x1600_d320");
        assert!(profile.calibrated);
        assert!(is_ldplayer_preset(900, 1600, 320));
    }
}
