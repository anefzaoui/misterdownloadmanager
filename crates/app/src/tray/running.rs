//! The second notification-area icon, for the downloads running now (IDM keeps two: one for the
//! program, one for its downloads in progress). Its picture is MDM's own, drawn here: a green
//! rounded square with a white down arrow on a tray bar, unlike the app icon.

/// The picture at `px` px (at least 1): straight-alpha RGBA, `px * px * 4` bytes.
pub fn picture(px: u32) -> Vec<u8> {
    let px = px.max(1);
    let mut rgba = Vec::with_capacity((px * px * 4) as usize);
    // 4 x 4 samples in each pixel give soft edges.
    const N: u32 = 4;
    for y in 0..px {
        for x in 0..px {
            let (mut green, mut white) = (0u32, 0u32);
            for sy in 0..N {
                for sx in 0..N {
                    let u = (x as f32 + (sx as f32 + 0.5) / N as f32) / px as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / N as f32) / px as f32;
                    if arrow(u, v) {
                        white += 1;
                    } else if square(u, v) {
                        green += 1;
                    }
                }
            }
            let total = N * N;
            let cover = green + white;
            if cover == 0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            // Green (46, 157, 79) and white, mixed by how much of the pixel each covers.
            let mix = |g: u32, w: u32| ((g * green + w * white) / cover) as u8;
            rgba.extend_from_slice(&[
                mix(46, 255),
                mix(157, 255),
                mix(79, 255),
                (cover * 255 / total) as u8,
            ]);
        }
    }
    rgba
}

/// The rounded square, in unit coordinates.
fn square(u: f32, v: f32) -> bool {
    let (lo, hi, r) = (0.04_f32, 0.96_f32, 0.2_f32);
    if u < lo || u > hi || v < lo || v > hi {
        return false;
    }
    let cx = u.clamp(lo + r, hi - r);
    let cy = v.clamp(lo + r, hi - r);
    (u - cx).powi(2) + (v - cy).powi(2) <= r * r
}

/// The down arrow and the bar under it, in unit coordinates.
fn arrow(u: f32, v: f32) -> bool {
    let stem = (0.42..=0.58).contains(&u) && (0.18..=0.5).contains(&v);
    // The head: from 0.22..0.78 wide at v = 0.48 to a point at v = 0.74.
    let head = (0.48..=0.74).contains(&v) && (u - 0.5).abs() <= 0.28 * (0.74 - v) / 0.26;
    let bar = (0.22..=0.78).contains(&u) && (0.8..=0.88).contains(&v);
    stem || head || bar
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picture_has_its_size_a_green_body_and_a_white_arrow() {
        for px in [16, 20, 24, 32, 48] {
            let p = picture(px);
            assert_eq!(p.len(), (px * px * 4) as usize);
            let at = |x: f32, y: f32| {
                let i = (((y * px as f32) as u32 * px + (x * px as f32) as u32) * 4) as usize;
                [p[i], p[i + 1], p[i + 2], p[i + 3]]
            };
            assert_eq!(at(0.5, 0.35)[..3], [255, 255, 255], "stem at {px}");
            assert_eq!(at(0.5, 0.35)[3], 255);
            assert_eq!(at(0.1, 0.5)[..3], [46, 157, 79], "body at {px}");
            assert_eq!(at(0.0, 0.0)[3], 0, "corner clear at {px}");
        }
    }

    #[test]
    fn a_zero_size_still_gives_one_pixel() {
        assert_eq!(picture(0).len(), 4);
    }
}
