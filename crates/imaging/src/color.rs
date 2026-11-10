use crate::DecodedImage;

/// One dominant color extracted from an image.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedColor {
    pub hex: String,
    pub ratio: f64,
}

/// Extracts at most three dominant colors using deterministic k-means in CIELAB.
///
/// Transparent pixels are ignored. Images without any opaque pixel produce an
/// empty list. The algorithm is deliberately small and dependency-free.
#[must_use]
pub fn extract_colors(image: &DecodedImage) -> Vec<ExtractedColor> {
    let step = (image.width.max(image.height) / 64).max(1);
    let mut pixels = Vec::new();
    let mut y = 0;
    while y < image.height {
        let mut x = 0;
        while x < image.width {
            let offset = ((y * image.width + x) * 4) as usize;
            let rgba = &image.rgba[offset..offset + 4];
            if rgba[3] >= 128 {
                pixels.push([rgba[0], rgba[1], rgba[2]]);
            }
            x += step;
        }
        y += step;
    }
    if pixels.is_empty() {
        return Vec::new();
    }

    let labs = pixels
        .iter()
        .map(|rgb| rgb_to_lab(*rgb))
        .collect::<Vec<_>>();
    let initial = initial_centers(&labs);
    let mut centers = initial;
    let mut assignment = vec![0_usize; labs.len()];
    for _ in 0..12 {
        for (index, lab) in labs.iter().enumerate() {
            assignment[index] = nearest(*lab, &centers);
        }
        let mut sums = vec![[0.0_f64; 3]; centers.len()];
        let mut counts = vec![0_usize; centers.len()];
        for (lab, cluster) in labs.iter().zip(&assignment) {
            for channel in 0..3 {
                sums[*cluster][channel] += lab[channel];
            }
            counts[*cluster] += 1;
        }
        for (index, center) in centers.iter_mut().enumerate() {
            if counts[index] > 0 {
                for channel in 0..3 {
                    center[channel] = sums[index][channel] / counts[index] as f64;
                }
            }
        }
    }

    let mut clusters = Vec::new();
    for (cluster, center) in centers.iter().enumerate() {
        let members = labs
            .iter()
            .zip(&assignment)
            .filter(|(_, assigned)| **assigned == cluster)
            .map(|(lab, _)| *lab)
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }
        clusters.push((center[0], center[1], center[2], members));
    }

    let mut merged: Vec<(f64, f64, f64, Vec<[f64; 3]>)> = Vec::new();
    for cluster in clusters {
        if let Some(existing) = merged.iter_mut().find(|other| {
            delta_e(
                [cluster.0, cluster.1, cluster.2],
                [other.0, other.1, other.2],
            ) < 12.0
        }) {
            existing.3.extend(cluster.3);
            let count = existing.3.len() as f64;
            existing.0 = existing.3.iter().map(|lab| lab[0]).sum::<f64>() / count;
            existing.1 = existing.3.iter().map(|lab| lab[1]).sum::<f64>() / count;
            existing.2 = existing.3.iter().map(|lab| lab[2]).sum::<f64>() / count;
        } else {
            merged.push(cluster);
        }
    }

    let total = pixels.len() as f64;
    let mut colors = merged
        .into_iter()
        .map(|(_, _, _, members)| {
            let count = members.len() as f64;
            let mut rgb = [0.0_f64; 3];
            for lab in &members {
                let pixel = lab_to_rgb(*lab);
                for channel in 0..3 {
                    rgb[channel] += f64::from(pixel[channel]);
                }
            }
            let rgb = [
                (rgb[0] / count).round().clamp(0.0, 255.0) as u8,
                (rgb[1] / count).round().clamp(0.0, 255.0) as u8,
                (rgb[2] / count).round().clamp(0.0, 255.0) as u8,
            ];
            ExtractedColor {
                hex: format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]),
                ratio: count / total,
            }
        })
        .collect::<Vec<_>>();
    colors.sort_by(|left, right| {
        right
            .ratio
            .total_cmp(&left.ratio)
            .then_with(|| left.hex.cmp(&right.hex))
    });
    colors.truncate(3);
    colors
}

fn initial_centers(labs: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mean = labs.iter().fold([0.0; 3], |mut sum, lab| {
        for channel in 0..3 {
            sum[channel] += lab[channel];
        }
        sum
    });
    let mean = [
        mean[0] / labs.len() as f64,
        mean[1] / labs.len() as f64,
        mean[2] / labs.len() as f64,
    ];
    let farthest = |from: &[[f64; 3]]| {
        labs.iter()
            .copied()
            .max_by(|left, right| {
                let left_distance = from
                    .iter()
                    .map(|center| delta_e(*left, *center))
                    .fold(f64::INFINITY, f64::min);
                let right_distance = from
                    .iter()
                    .map(|center| delta_e(*right, *center))
                    .fold(f64::INFINITY, f64::min);
                left_distance.total_cmp(&right_distance)
            })
            .unwrap_or(mean)
    };
    let first = farthest(&[mean]);
    let second = farthest(&[first]);
    let third = farthest(&[first, second]);
    vec![first, second, third]
}

fn nearest(lab: [f64; 3], centers: &[[f64; 3]]) -> usize {
    let mut best = 0;
    let mut best_distance = f64::INFINITY;
    for (index, center) in centers.iter().enumerate() {
        let distance = delta_e(lab, *center);
        if distance < best_distance {
            best = index;
            best_distance = distance;
        }
    }
    best
}

/// CIE76 color difference between two CIELAB colors.
#[must_use]
pub fn delta_e(left: [f64; 3], right: [f64; 3]) -> f64 {
    let dl = left[0] - right[0];
    let da = left[1] - right[1];
    let db = left[2] - right[2];
    (dl * dl + da * da + db * db).sqrt()
}

/// Converts an sRGB color to CIELAB (D65).
#[must_use]
pub fn rgb_to_lab(rgb: [u8; 3]) -> [f64; 3] {
    let linear = |value: u8| {
        let value = f64::from(value) / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let r = linear(rgb[0]);
    let g = linear(rgb[1]);
    let b = linear(rgb[2]);
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |value: f64| {
        if value > 0.008_856_452 {
            value.cbrt()
        } else {
            (903.3 * value + 16.0) / 116.0
        }
    };
    [
        116.0 * f(y) - 16.0,
        500.0 * (f(x) - f(y)),
        200.0 * (f(y) - f(z)),
    ]
}

fn lab_to_rgb(lab: [f64; 3]) -> [u8; 3] {
    let fy = (lab[0] + 16.0) / 116.0;
    let fx = fy + lab[1] / 500.0;
    let fz = fy - lab[2] / 200.0;
    let inverse = |value: f64| {
        let cube = value * value * value;
        if cube > 0.008_856_452 {
            cube
        } else {
            (116.0 * value - 16.0) / 903.3
        }
    };
    let x = inverse(fx) * 0.950_47;
    let y = inverse(fy);
    let z = inverse(fz) * 1.088_83;
    let r = 3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z;
    let g = -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z;
    let b = 0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z;
    let encode = |value: f64| {
        let value = value.clamp(0.0, 1.0);
        let encoded = if value <= 0.003_130_8 {
            12.92 * value
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round().clamp(0.0, 255.0) as u8
    };
    [encode(r), encode(g), encode(b)]
}
