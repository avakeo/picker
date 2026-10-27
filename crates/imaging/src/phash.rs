use crate::decode::DecodedImage;

/// Width of the difference hash grid.
pub const DHASH_WIDTH: u32 = 9;
/// Height of the difference hash grid.
pub const DHASH_HEIGHT: u32 = 8;
/// Maximum Hamming distance treated as an approximate duplicate by default.
pub const DEFAULT_NEAR_DUPLICATE_DISTANCE: u32 = 10;

/// Computes a 64-bit difference hash from a decoded image.
///
/// The image is converted to grayscale and resized to 9x8. Each bit records
/// whether a pixel is brighter than the pixel to its right. This is a local
/// implementation because the algorithm is small and avoids an extra
/// dependency (design principle 5).
#[must_use]
pub fn dhash(image: &DecodedImage) -> [u8; 8] {
    let Some(rgba) = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
    else {
        return [0; 8];
    };
    let grayscale = image::imageops::resize(
        &image::DynamicImage::ImageRgba8(rgba).to_luma8(),
        DHASH_WIDTH,
        DHASH_HEIGHT,
        image::imageops::FilterType::Triangle,
    );
    let mut hash = [0_u8; 8];
    for y in 0..DHASH_HEIGHT {
        for x in 0..(DHASH_WIDTH - 1) {
            let left = grayscale.get_pixel(x, y)[0];
            let right = grayscale.get_pixel(x + 1, y)[0];
            if left > right {
                let bit = (y * (DHASH_WIDTH - 1) + x) as usize;
                hash[bit / 8] |= 1 << (7 - (bit % 8));
            }
        }
    }
    hash
}

/// Returns the number of differing bits between two hashes.
#[must_use]
pub fn hamming_distance(left: &[u8; 8], right: &[u8; 8]) -> u32 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left ^ right).count_ones())
        .sum()
}

/// Returns whether two hashes are within the approximate-duplicate threshold.
#[must_use]
pub fn is_near_duplicate(left: &[u8; 8], right: &[u8; 8], threshold: u32) -> bool {
    hamming_distance(left, right) <= threshold
}
