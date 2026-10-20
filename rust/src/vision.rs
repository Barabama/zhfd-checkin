use anyhow::Result;
use image::{DynamicImage, imageops::FilterType};
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ButtonState {
    Gray,
    Locating,
    Ready,
    Success,
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ButtonBox {
    pub cx: u32,
    pub cy: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ButtonAnalysis {
    pub state: ButtonState,
    pub button: Option<ButtonBox>,
    pub dominant_h: f32,
    pub color_coverage: f32,
    pub white_rows: Vec<(u32, u32, u32)>,
    pub white_count: u32,
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    let mut h = if d == 0.0 {
        0.0
    } else if (max - rf).abs() < f32::EPSILON {
        60.0 * (((gf - bf) / d) % 6.0)
    } else if (max - gf).abs() < f32::EPSILON {
        60.0 * ((bf - rf) / d + 2.0)
    } else {
        60.0 * ((rf - gf) / d + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    (
        h / 2.0,
        if max == 0.0 { 0.0 } else { d / max * 255.0 },
        max * 255.0,
    )
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    values[values.len() / 2]
}

fn in_circle(x: i32, y: i32, radius: f32) -> bool {
    (x as f32).powi(2) + (y as f32).powi(2) <= radius.powi(2)
}

pub fn classify_button(image: &DynamicImage, button: ButtonBox) -> ButtonAnalysis {
    let cropped = crop(image, button);
    if cropped.width() == 0 || cropped.height() == 0 {
        return ButtonAnalysis {
            state: ButtonState::Unknown,
            button: Some(button),
            dominant_h: 0.0,
            color_coverage: 0.0,
            white_rows: vec![],
            white_count: 0,
        };
    }
    let normalized = image::imageops::resize(&cropped.to_rgb8(), 315, 315, FilterType::Triangle);
    let mut colored_h = Vec::new();
    let mut median_h_values = Vec::new();
    let mut colored = 0u32;
    let mut total = 0u32;
    let mut white_count = 0u32;
    let radius = 315.0 * 0.43;
    let mut projection = vec![0u32; 315];
    for y in 0..315i32 {
        for x in 0..315i32 {
            let dx = x - 157;
            let dy = y - 157;
            if !in_circle(dx, dy, radius) {
                continue;
            }
            total += 1;
            let px = normalized.get_pixel(x as u32, y as u32);
            let (h, s, v) = rgb_to_hsv(px[0], px[1], px[2]);
            median_h_values.push(h);
            if s > 40.0 && v > 60.0 {
                colored += 1;
                colored_h.push(h);
            }
            if s < 80.0 && v > 235.0 {
                white_count += 1;
                projection[y as usize] += 1;
            }
        }
    }
    let color_coverage = colored as f32 / total.max(1) as f32;
    let dominant_h = median(&mut colored_h);
    let median_h = median(&mut median_h_values);
    let mut rows: Vec<(u32, u32, u32)> = Vec::new();
    for (y, value) in projection.into_iter().enumerate() {
        if value <= 3 {
            continue;
        }
        let y = y as u32;
        if let Some(last) = rows.last_mut()
            && y - last.1 <= 4
        {
            last.1 = y;
            last.2 += value;
            continue;
        }
        rows.push((y, y, value));
    }
    rows.retain(|(_, _, sum)| *sum >= 150);
    let state = if color_coverage > 0.35 && (35.0..=85.0).contains(&dominant_h) {
        ButtonState::Success
    } else if color_coverage > 0.55 && (95.0..=130.0).contains(&dominant_h) {
        if rows.len() >= 2 && rows.iter().any(|(start, _, _)| *start >= 157) {
            ButtonState::Ready
        } else {
            ButtonState::Locating
        }
    } else if median_h >= 0.0 && color_coverage < 0.05 && median_h.is_finite() {
        // A low-saturation gray button is accepted only in the same brightness
        // band as the known "无法签到" sample.
        let mut values = Vec::new();
        for y in 0..315u32 {
            for x in 0..315u32 {
                let dx = x as i32 - 157;
                let dy = y as i32 - 157;
                if in_circle(dx, dy, radius) {
                    let p = normalized.get_pixel(x, y);
                    values.push(rgb_to_hsv(p[0], p[1], p[2]).2);
                }
            }
        }
        let v = median(&mut values);
        if (80.0..230.0).contains(&v) {
            ButtonState::Gray
        } else {
            ButtonState::Unknown
        }
    } else {
        ButtonState::Unknown
    };
    ButtonAnalysis {
        state,
        button: Some(button),
        dominant_h,
        color_coverage,
        white_rows: rows,
        white_count,
    }
}

pub fn crop(image: &DynamicImage, button: ButtonBox) -> DynamicImage {
    let half_w = button.width / 2;
    let half_h = button.height / 2;
    let x = button.cx.saturating_sub(half_w);
    let y = button.cy.saturating_sub(half_h);
    let max_w = image.width().saturating_sub(x);
    let max_h = image.height().saturating_sub(y);
    let w = button.width.min(max_w).max(1);
    let h = button.height.min(max_h).max(1);
    image.crop_imm(x, y, w, h)
}

/// The minimum accent-blue pixel count and fraction for a consent checkbox to
/// count as checked. The unchecked box is white with a light-grey outline, so
/// the checked state is a large blue fraction at every resolution. The
/// fraction is the real gate; the absolute floor only rejects a few stray
/// antialiased pixels. Both are deliberately conservative: an ambiguous crop
/// must fail closed.
const CONSENT_ACCENT_MIN_PIXELS: u64 = 4;
const CONSENT_ACCENT_MIN_FRACTION: f32 = 0.02;

fn is_accent_blue(r: u8, g: u8, b: u8) -> bool {
    b >= 150 && b as i32 >= r as i32 + 40 && g >= 60
}

/// Count accent-blue pixels inside `rect` and the total pixels considered.
///
/// The rectangle is clamped to the image; an empty or out-of-bounds rectangle
/// yields `(0, 0)`.
pub fn accent_pixels(image: &DynamicImage, rect: (u32, u32, u32, u32)) -> (u64, u64) {
    let (left, top, right, bottom) = rect;
    let x = left.min(image.width());
    let y = top.min(image.height());
    let w = right.min(image.width()).saturating_sub(x);
    let h = bottom.min(image.height()).saturating_sub(y);
    if w == 0 || h == 0 {
        return (0, 0);
    }
    let rgb = image.to_rgb8();
    let mut accent = 0u64;
    for yy in y..y + h {
        for xx in x..x + w {
            let pixel = rgb.get_pixel(xx, yy);
            if is_accent_blue(pixel[0], pixel[1], pixel[2]) {
                accent += 1;
            }
        }
    }
    (accent, (w as u64) * (h as u64))
}

/// Visual confirmation that a consent checkbox is ticked.
///
/// Hierarchy nodes on this App expose no `checked` attribute, so the only
/// available signal is the accent colour drawn inside the node's bounds. The
/// caller must treat `false` as "not confirmed" and must not tap login.
pub fn consent_checkbox_checked(image: &DynamicImage, rect: (u32, u32, u32, u32)) -> bool {
    let (accent, total) = accent_pixels(image, rect);
    if total == 0 {
        return false;
    }
    accent >= CONSENT_ACCENT_MIN_PIXELS
        && (accent as f32 / total as f32) >= CONSENT_ACCENT_MIN_FRACTION
}

pub fn fallback_button(image: &DynamicImage, profile: &crate::profile::Profile) -> ButtonBox {
    let w = image.width();
    let h = image.height();
    ButtonBox {
        cx: (w as f32 * profile.button_center_ratio.0) as u32,
        cy: (h as f32 * profile.button_center_ratio.1) as u32,
        width: (w as f32 * profile.button_box_ratio.0) as u32,
        height: (h as f32 * profile.button_box_ratio.1) as u32,
    }
}

pub fn find_colored_button(image: &DynamicImage) -> Option<ButtonBox> {
    let rgb = image.to_rgb8();
    let w = rgb.width() as usize;
    let h = rgb.height() as usize;
    let step = ((w.min(h) / 300).max(2)) as u32;
    let x_count = (rgb.width() / step).max(1) as usize;
    let y_start = (rgb.height() as f32 * 0.35) as u32;
    let y_end = (rgb.height() as f32 * 0.95) as u32;
    let y_count = ((y_end.saturating_sub(y_start)) / step).max(1) as usize;
    let mut grid = vec![false; x_count * y_count];
    for gy in 0..y_count {
        for gx in 0..x_count {
            let x = (gx as u32 * step + step / 2).min(rgb.width() - 1);
            let y = (y_start + gy as u32 * step + step / 2).min(rgb.height() - 1);
            let p = rgb.get_pixel(x, y);
            let (hue, sat, val) = rgb_to_hsv(p[0], p[1], p[2]);
            grid[gy * x_count + gx] = val > 60.0
                && sat > 80.0
                && (((95.0..=130.0).contains(&hue)) || ((35.0..=85.0).contains(&hue)));
        }
    }
    let mut seen = vec![false; grid.len()];
    // Real sign-in circles occupy a substantial fraction of the viewport.
    // Small home-page service icons must not become button candidates.
    let min_button_edge = ((w.min(h) as f32) * 0.20).round() as u32;
    let mut best: Option<(usize, usize, usize, usize, usize)> = None;
    for sy in 0..y_count {
        for sx in 0..x_count {
            let index = sy * x_count + sx;
            if !grid[index] || seen[index] {
                continue;
            }
            let mut queue = VecDeque::new();
            queue.push_back((sx, sy));
            seen[index] = true;
            let mut count = 0usize;
            let (mut min_x, mut max_x, mut min_y, mut max_y) = (sx, sx, sy, sy);
            while let Some((x, y)) = queue.pop_front() {
                count += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
                for (nx, ny) in [
                    (x.wrapping_sub(1), y),
                    (x + 1, y),
                    (x, y.wrapping_sub(1)),
                    (x, y + 1),
                ] {
                    if nx < x_count && ny < y_count {
                        let ni = ny * x_count + nx;
                        if grid[ni] && !seen[ni] {
                            seen[ni] = true;
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            let bw = (max_x - min_x + 1) as u32 * step;
            let bh = (max_y - min_y + 1) as u32 * step;
            let ratio = bw as f32 / bh.max(1) as f32;
            if count >= 40
                && bw >= min_button_edge
                && bh >= min_button_edge
                && (0.55..=1.8).contains(&ratio)
                && best.as_ref().map(|b| count > b.0).unwrap_or(true)
            {
                best = Some((count, min_x, max_x, min_y, max_y));
            }
        }
    }
    best.map(|(_, min_x, max_x, min_y, max_y)| {
        let cx = ((min_x + max_x) as u32 * step / 2).min(rgb.width() - 1);
        let cy = (y_start + (min_y + max_y) as u32 * step / 2).min(rgb.height() - 1);
        let width = ((max_x - min_x + 1) as u32 * step).max(1);
        let height = ((max_y - min_y + 1) as u32 * step).max(1);
        ButtonBox {
            cx,
            cy,
            width,
            height,
        }
    })
}

pub fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    Ok(image::load_from_memory(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> DynamicImage {
        let bytes: &[u8] = match name {
            "gray" => include_bytes!("../../autojs6/fixtures/gray.png"),
            "locating" => include_bytes!("../../autojs6/fixtures/locating.png"),
            "ready" => include_bytes!("../../autojs6/fixtures/ready.png"),
            "success" => include_bytes!("../../autojs6/fixtures/success.png"),
            _ => panic!("unknown fixture"),
        };
        decode(bytes).expect("fixture decode")
    }

    #[test]
    fn ignores_small_home_service_icon() {
        let mut image = image::RgbImage::from_pixel(900, 1600, image::Rgb([255, 255, 255]));
        for y in 1200..1296 {
            for x in 76..172 {
                image.put_pixel(x, y, image::Rgb([25, 145, 235]));
            }
        }
        let image = DynamicImage::ImageRgb8(image);
        assert!(find_colored_button(&image).is_none());
    }

    #[test]
    fn classifies_python_baseline_fixtures() {
        for (name, expected) in [
            ("gray", ButtonState::Gray),
            ("locating", ButtonState::Locating),
            ("ready", ButtonState::Ready),
            ("success", ButtonState::Success),
        ] {
            let image = fixture(name);
            let button = ButtonBox {
                cx: 540,
                cy: 1317,
                width: 315,
                height: 315,
            };
            let actual = classify_button(&image, button).state;
            assert_eq!(actual, expected, "fixture {name}");
        }
    }

    #[test]
    fn consent_checkbox_requires_visible_accent() {
        let rect = (10, 10, 50, 64);
        // Unchecked: white box with a light-grey outline (observed baseline).
        let mut unchecked = image::RgbImage::from_pixel(64, 80, image::Rgb([255, 255, 255]));
        for x in 10..50 {
            unchecked.put_pixel(x, 10, image::Rgb([233, 233, 233]));
            unchecked.put_pixel(x, 63, image::Rgb([221, 221, 221]));
        }
        let unchecked = DynamicImage::ImageRgb8(unchecked);
        let (accent, total) = accent_pixels(&unchecked, rect);
        assert_eq!(total, 40 * 54);
        assert_eq!(accent, 0);
        assert!(!consent_checkbox_checked(&unchecked, rect));

        // Checked: a blue tick/fill inside the same bounds.
        let mut checked = image::RgbImage::from_pixel(64, 80, image::Rgb([255, 255, 255]));
        for y in 18..58 {
            for x in 18..42 {
                checked.put_pixel(x, y, image::Rgb([13, 148, 255]));
            }
        }
        let checked = DynamicImage::ImageRgb8(checked);
        assert!(consent_checkbox_checked(&checked, rect));
    }

    #[test]
    fn consent_checkbox_scales_to_lower_resolution_profiles() {
        // A 20x28 crop approximates the checkbox at the 540x960@240 preset.
        // The fraction-based threshold must still accept a proportional tick.
        let rect = (4, 4, 24, 32);
        let mut checked = image::RgbImage::from_pixel(32, 40, image::Rgb([255, 255, 255]));
        for y in 8..28 {
            for x in 8..20 {
                checked.put_pixel(x, y, image::Rgb([13, 148, 255]));
            }
        }
        let checked = DynamicImage::ImageRgb8(checked);
        assert!(consent_checkbox_checked(&checked, rect));

        // A handful of stray blue pixels is below the 2% fraction and must not
        // be read as checked.
        let mut sparse = image::RgbImage::from_pixel(32, 40, image::Rgb([255, 255, 255]));
        for (x, y) in [(10, 10), (11, 10), (10, 11)] {
            sparse.put_pixel(x, y, image::Rgb([13, 148, 255]));
        }
        let sparse = DynamicImage::ImageRgb8(sparse);
        assert!(!consent_checkbox_checked(&sparse, rect));
    }

    #[test]
    fn consent_checkbox_rejects_empty_rect() {
        let image = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            10,
            10,
            image::Rgb([13, 148, 255]),
        ));
        assert_eq!(accent_pixels(&image, (20, 20, 30, 30)), (0, 0));
        assert!(!consent_checkbox_checked(&image, (20, 20, 30, 30)));
    }
}
