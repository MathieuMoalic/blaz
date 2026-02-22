use image::DynamicImage;
use image::GenericImageView;
use webp::Encoder as WebpEncoder;

pub const FULL_WEBP_QUALITY: f32 = 90.0;
pub const THUMB_WEBP_QUALITY: f32 = 10.0;
pub const THUMB_MAX_DIM: u32 = 1024;

/// # Errors
///
/// Returns Err if the image incoding fails
pub fn to_full_and_thumb_webp(img: &DynamicImage) -> std::io::Result<(Vec<u8>, Vec<u8>)> {
    // full
    let full_mem = WebpEncoder::from_image(img)
        .map_err(err_other)?
        .encode(FULL_WEBP_QUALITY);

    // thumb
    let (w, h) = img.dimensions();
    let thumb_img = if w <= THUMB_MAX_DIM && h <= THUMB_MAX_DIM {
        img.clone()
    } else {
        img.resize(
            THUMB_MAX_DIM,
            THUMB_MAX_DIM,
            image::imageops::FilterType::Triangle,
        )
    };
    let thumb_mem = WebpEncoder::from_image(&thumb_img)
        .map_err(err_other)?
        .encode(THUMB_WEBP_QUALITY);

    Ok((full_mem.to_vec(), thumb_mem.to_vec()))
}

fn err_other<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::other(e.to_string())
}
