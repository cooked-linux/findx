//! Image decoding, EXIF orientation normalization, and aspect-ratio scaling.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use image::imageops::FilterType;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits, RgbaImage};

use super::ThumbnailError;

/// Maximum allocation limit (256 MB) to prevent decompression bombs.
const MAX_ALLOC_LIMIT_BYTES: u64 = 256 * 1024 * 1024;
/// Maximum dimension limit (16,384 px) for width/height.
const MAX_DIMENSION_PIXELS: u32 = 16_384;

/// Applies EXIF orientation rotation and flip transformations to a [`DynamicImage`].
///
/// Supports EXIF orientation values 1 through 8:
/// - 1: Normal
/// - 2: Flip Horizontal
/// - 3: Rotate 180°
/// - 4: Flip Vertical
/// - 5: Rotate 90° CW and Flip Horizontal
/// - 6: Rotate 90° CW
/// - 7: Rotate 270° CW and Flip Horizontal
/// - 8: Rotate 270° CW
pub fn apply_exif_orientation(img: &mut DynamicImage, orientation_tag: u32) {
    let orientation = match orientation_tag {
        2 => Orientation::FlipHorizontal,
        3 => Orientation::Rotate180,
        4 => Orientation::FlipVertical,
        5 => Orientation::Rotate90FlipH,
        6 => Orientation::Rotate90,
        7 => Orientation::Rotate270FlipH,
        8 => Orientation::Rotate270,
        _ => Orientation::NoTransforms,
    };
    img.apply_orientation(orientation);
}

/// Decodes an image file, normalizes its orientation, and downsamples it to fit
/// within `max_dim x max_dim` preserving aspect ratio.
///
/// Returns a 32-bit RGBA image buffer.
///
/// # Errors
///
/// Returns [`ThumbnailError::Io`] if the file cannot be opened.
/// Returns [`ThumbnailError::Image`] if decoding or format conversion fails.
pub fn generate_thumbnail_image(
    file_path: &Path,
    max_dim: u32,
) -> Result<RgbaImage, ThumbnailError> {
    let file = File::open(file_path)?;
    let buf_reader = BufReader::new(file);

    let mut reader = ImageReader::new(buf_reader)
        .with_guessed_format()
        .map_err(ThumbnailError::Io)?;

    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC_LIMIT_BYTES);
    limits.max_image_width = Some(MAX_DIMENSION_PIXELS);
    limits.max_image_height = Some(MAX_DIMENSION_PIXELS);
    reader.limits(limits);

    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut dynamic_image = DynamicImage::from_decoder(decoder)?;

    dynamic_image.apply_orientation(orientation);

    // Freedesktop spec: downsample preserving aspect ratio, never scale up
    let (w, h) = (dynamic_image.width(), dynamic_image.height());
    let scaled = if w > max_dim || h > max_dim {
        dynamic_image.resize(max_dim, max_dim, FilterType::Lanczos3)
    } else {
        dynamic_image
    };

    Ok(scaled.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};

    #[test]
    fn test_apply_exif_orientations() {
        // Create 2x1 image: (0,0) = Red, (1,0) = Blue
        let mut base = RgbaImage::new(2, 1);
        base.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        base.put_pixel(1, 0, Rgba([0, 0, 255, 255]));

        // Orientation 1: Normal
        let mut img1 = DynamicImage::ImageRgba8(base.clone());
        apply_exif_orientation(&mut img1, 1);
        assert_eq!(img1.dimensions(), (2, 1));
        assert_eq!(img1.to_rgba8().get_pixel(0, 0), &Rgba([255, 0, 0, 255]));

        // Orientation 6: Rotate 90 CW (becomes 1x2, top is Red, bottom is Blue)
        let mut img6 = DynamicImage::ImageRgba8(base.clone());
        apply_exif_orientation(&mut img6, 6);
        assert_eq!(img6.dimensions(), (1, 2));
        assert_eq!(img6.to_rgba8().get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(img6.to_rgba8().get_pixel(0, 1), &Rgba([0, 0, 255, 255]));

        // Orientation 8: Rotate 270 CW (becomes 1x2, top is Blue, bottom is Red)
        let mut img8 = DynamicImage::ImageRgba8(base.clone());
        apply_exif_orientation(&mut img8, 8);
        assert_eq!(img8.dimensions(), (1, 2));
        assert_eq!(img8.to_rgba8().get_pixel(0, 0), &Rgba([0, 0, 255, 255]));
        assert_eq!(img8.to_rgba8().get_pixel(0, 1), &Rgba([255, 0, 0, 255]));

        // Orientation 2: Flip Horizontal (becomes 2x1, left is Blue, right is Red)
        let mut img2 = DynamicImage::ImageRgba8(base);
        apply_exif_orientation(&mut img2, 2);
        assert_eq!(img2.dimensions(), (2, 1));
        assert_eq!(img2.to_rgba8().get_pixel(0, 0), &Rgba([0, 0, 255, 255]));
        assert_eq!(img2.to_rgba8().get_pixel(1, 0), &Rgba([255, 0, 0, 255]));
    }
}
