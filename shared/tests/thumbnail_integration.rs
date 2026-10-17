//! Integration tests for Freedesktop Thumbnail Cache Pipeline.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use image::{GenericImageView, ImageBuffer, Rgba, RgbaImage};
use shared::thumbnail::{
    ThumbnailCache, ThumbnailSize, apply_exif_orientation, canonical_file_uri,
    compute_thumbnail_hash, read_thumbnail_metadata,
};

fn create_mock_png(path: &Path, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
    let mut img: RgbaImage = ImageBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = u8::try_from((x * 255) / width.max(1)).unwrap_or(0);
        let g = u8::try_from((y * 255) / height.max(1)).unwrap_or(0);
        *pixel = Rgba([r, g, 128, 255]);
    }
    img.save(path)?;
    Ok(())
}

fn create_mock_jpeg(
    path: &Path,
    width: u32,
    height: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut img: image::RgbImage = ImageBuffer::new(width, height);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = u8::try_from((x * 200) / width.max(1)).unwrap_or(0);
        let g = u8::try_from((y * 200) / height.max(1)).unwrap_or(0);
        *pixel = image::Rgb([r, g, 200]);
    }
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut writer);
    encoder.encode(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)?;
    Ok(())
}

#[test]
fn test_canonical_uri_and_freedesktop_md5() -> Result<(), Box<dyn std::error::Error>> {
    // Standard test vectors
    let uri1 = "file:///home/user/images/photo.png";
    assert_eq!(
        compute_thumbnail_hash(uri1),
        "510f26f2e9a639f7e27797d787ba9e47"
    );

    let uri2 = "file:///home/jens/photos/me.jpg";
    assert_eq!(
        compute_thumbnail_hash(uri2),
        "d2707135b6fa80597d0fcc4c59f84b87"
    );

    // Spaces and Unicode characters
    let unicode_path = Path::new("/home/user/My Photos/résumé & café.png");
    let uri_unicode = canonical_file_uri(unicode_path)?;
    assert_eq!(
        uri_unicode,
        "file:///home/user/My%20Photos/r%C3%A9sum%C3%A9%20%26%20caf%C3%A9.png"
    );
    assert_eq!(compute_thumbnail_hash(&uri_unicode).len(), 32);
    Ok(())
}

#[test]
fn test_mock_png_and_jpeg_thumbnail_generation_and_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = tempfile::tempdir()?;
    let temp_cache = tempfile::tempdir()?;

    let cache = ThumbnailCache::with_cache_dir(temp_cache.path().to_path_buf());

    // 1. Mock PNG (400x200)
    let png_path = temp_workspace.path().join("source_image.png");
    create_mock_png(&png_path, 400, 200)?;

    let thumb_path = cache.get_or_generate(&png_path, ThumbnailSize::Normal, None)?;

    assert!(thumb_path.is_file());
    assert!(thumb_path.starts_with(temp_cache.path().join("normal")));

    // Read image back and check downsampled dimensions
    let thumb_img = image::open(&thumb_path)?;
    assert_eq!(thumb_img.dimensions(), (128, 64)); // 400x200 scaled to max 128x128 preserving 2:1 aspect

    // Read metadata and verify standard chunks
    let meta = read_thumbnail_metadata(&thumb_path)?;
    let expected_uri = canonical_file_uri(&png_path)?;
    assert_eq!(meta.uri.as_deref(), Some(expected_uri.as_str()));
    assert_eq!(meta.mime_type.as_deref(), Some("image/png"));
    assert_eq!(meta.software.as_deref(), Some("findx 0.1.0"));
    assert!(meta.mtime.is_some());
    assert!(meta.size.is_some());

    // 2. Mock JPEG (300x300)
    let jpg_path = temp_workspace.path().join("source_image.jpg");
    create_mock_jpeg(&jpg_path, 300, 300)?;

    let thumb_jpg = cache.get_or_generate(&jpg_path, ThumbnailSize::Large, Some("image/jpeg"))?;

    assert!(thumb_jpg.is_file());
    assert!(thumb_jpg.starts_with(temp_cache.path().join("large")));

    let thumb_jpg_img = image::open(&thumb_jpg)?;
    assert_eq!(thumb_jpg_img.dimensions(), (256, 256));

    let jpg_meta = read_thumbnail_metadata(&thumb_jpg)?;
    assert_eq!(jpg_meta.mime_type.as_deref(), Some("image/jpeg"));
    Ok(())
}

#[test]
fn test_cache_hit_and_invalidation_on_mtime_change() -> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = tempfile::tempdir()?;
    let temp_cache = tempfile::tempdir()?;

    let cache = ThumbnailCache::with_cache_dir(temp_cache.path().to_path_buf());
    let img_path = temp_workspace.path().join("test_cache.png");
    create_mock_png(&img_path, 200, 200)?;

    // Initial generation
    let thumb1 = cache.get_or_generate(&img_path, ThumbnailSize::Normal, None)?;

    let meta1 = read_thumbnail_metadata(&thumb1)?;

    // Cache hit: should return the same file without modifying it
    let thumb2 = cache.get_or_generate(&img_path, ThumbnailSize::Normal, None)?;
    assert_eq!(thumb1, thumb2);

    let meta2 = read_thumbnail_metadata(&thumb2)?;
    assert_eq!(meta1.mtime, meta2.mtime);

    // Invalidate cache: update source file mtime to the future
    let future_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(60, |dur| dur.as_secs().saturating_add(60));
    let time_sec_i64 = i64::try_from(future_secs).unwrap_or(60);
    let time_spec = nix::sys::time::TimeSpec::new(time_sec_i64, 0);
    nix::sys::stat::utimensat(
        None,
        &img_path,
        &time_spec,
        &time_spec,
        nix::sys::stat::UtimensatFlags::NoFollowSymlink,
    )?;

    // Next lookup must trigger regeneration and update Thumb::MTime
    let thumb3 = cache.get_or_generate(&img_path, ThumbnailSize::Normal, None)?;

    let meta3 = read_thumbnail_metadata(&thumb3)?;
    assert_ne!(meta1.mtime, meta3.mtime);
    Ok(())
}

#[test]
fn test_corrupt_file_writes_fail_marker() -> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = tempfile::tempdir()?;
    let temp_cache = tempfile::tempdir()?;

    let cache = ThumbnailCache::with_cache_dir(temp_cache.path().to_path_buf());
    let corrupt_path = temp_workspace.path().join("broken.png");
    std::fs::write(&corrupt_path, b"NOT_A_VALID_IMAGE_HEADER")?;

    // First attempt should fail and write marker to fail/
    let result = cache.get_or_generate(&corrupt_path, ThumbnailSize::Normal, Some("image/png"));
    assert!(result.is_err());

    let uri = canonical_file_uri(&corrupt_path)?;
    let hash = compute_thumbnail_hash(&uri);
    let fail_file = temp_cache.path().join("fail").join(format!("{hash}.png"));
    assert!(fail_file.is_file(), "failure marker was not written");

    // Second attempt should fail early
    let second_result =
        cache.get_or_generate(&corrupt_path, ThumbnailSize::Normal, Some("image/png"));
    assert!(second_result.is_err());
    Ok(())
}

#[test]
fn test_exif_orientation_correction_matrix() {
    // Create an asymmetric 4x2 image to test rotations and flips
    let mut base = RgbaImage::new(4, 2);
    // Mark (0, 0) as Red, all others black
    base.put_pixel(0, 0, Rgba([255, 0, 0, 255]));

    // Orientation 3: Rotate 180° -> (0,0) moves to (3,1)
    let mut img3 = image::DynamicImage::ImageRgba8(base.clone());
    apply_exif_orientation(&mut img3, 3);
    assert_eq!(img3.dimensions(), (4, 2));
    assert_eq!(img3.to_rgba8().get_pixel(3, 1), &Rgba([255, 0, 0, 255]));

    // Orientation 6: Rotate 90° CW -> Dimensions become 2x4, (0,0) moves to (1,0)
    let mut img6 = image::DynamicImage::ImageRgba8(base.clone());
    apply_exif_orientation(&mut img6, 6);
    assert_eq!(img6.dimensions(), (2, 4));
    assert_eq!(img6.to_rgba8().get_pixel(1, 0), &Rgba([255, 0, 0, 255]));

    // Orientation 8: Rotate 270° CW -> Dimensions become 2x4, (0,0) moves to (0,3)
    let mut img8 = image::DynamicImage::ImageRgba8(base.clone());
    apply_exif_orientation(&mut img8, 8);
    assert_eq!(img8.dimensions(), (2, 4));
    assert_eq!(img8.to_rgba8().get_pixel(0, 3), &Rgba([255, 0, 0, 255]));

    // Orientation 4: Flip Vertical -> Dimensions 4x2, (0,0) moves to (0,1)
    let mut img4 = image::DynamicImage::ImageRgba8(base);
    apply_exif_orientation(&mut img4, 4);
    assert_eq!(img4.dimensions(), (4, 2));
    assert_eq!(img4.to_rgba8().get_pixel(0, 1), &Rgba([255, 0, 0, 255]));
}

#[test]
fn test_multi_tier_size_buckets() -> Result<(), Box<dyn std::error::Error>> {
    let temp_workspace = tempfile::tempdir()?;
    let temp_cache = tempfile::tempdir()?;

    let cache = ThumbnailCache::with_cache_dir(temp_cache.path().to_path_buf());
    let img_path = temp_workspace.path().join("large_canvas.png");
    create_mock_png(&img_path, 1000, 1000)?;

    let normal_path = cache.get_or_generate(&img_path, ThumbnailSize::Normal, None)?;
    let normal_img = image::open(&normal_path)?;
    assert_eq!(normal_img.dimensions(), (128, 128));

    let large_path = cache.get_or_generate(&img_path, ThumbnailSize::Large, None)?;
    let large_img = image::open(&large_path)?;
    assert_eq!(large_img.dimensions(), (256, 256));

    let xlarge_path = cache.get_or_generate(&img_path, ThumbnailSize::XLarge, None)?;
    let xlarge_img = image::open(&xlarge_path)?;
    assert_eq!(xlarge_img.dimensions(), (512, 512));
    Ok(())
}
