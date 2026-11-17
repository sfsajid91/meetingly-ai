//! Primary display capture scaled and JPEG-compressed for vision requests.

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use std::io::Cursor;

const MAX_WIDTH: u32 = 1280;
const MAX_HEIGHT: u32 = 800;
const JPEG_QUALITY: u8 = 70;
const MIN_QUALITY: u8 = 35;
const MAX_BYTES: usize = 350 * 1024;

#[derive(Debug, Clone)]
pub struct CapturedScreen {
    pub base64: String,
    pub width: u32,
    pub height: u32,
    #[cfg_attr(not(test), allow(dead_code))]
    pub bytes: usize,
}

#[cfg(target_os = "linux")]
pub async fn capture_primary_display() -> Result<CapturedScreen, String> {
    use ashpd::desktop::screenshot::Screenshot;

    let request = Screenshot::request()
        .interactive(false)
        .modal(false)
        .send()
        .await
        .map_err(|e| format!("Screen capture request failed: {e}"))?;
    let response = request
        .response()
        .map_err(|e| format!("Screen capture was cancelled or denied: {e}"))?;
    let url = url::Url::parse(response.uri().as_str()).map_err(|e| format!("Invalid screenshot URI: {e}"))?;
    let path = url
        .to_file_path()
        .map_err(|()| format!("Screenshot URI is not a local file: {}", response.uri()))?;
    let bytes = tokio::fs::read(&path).await.map_err(|e| format!("Could not read screenshot file: {e}"))?;
    let _ = tokio::fs::remove_file(&path).await;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("Could not decode screenshot: {e}"))?;
    encode_jpeg_under_limit(img)
}

#[cfg(not(target_os = "linux"))]
pub async fn capture_primary_display() -> Result<CapturedScreen, String> {
    tokio::task::spawn_blocking(|| {
        let monitors = xcap::Monitor::all().map_err(|e| format!("Screen capture failed: {e}"))?;
        let primary = monitors
            .iter()
            .find(|m| m.is_primary().unwrap_or(false))
            .or_else(|| monitors.first())
            .ok_or_else(|| "No display found for screen capture.".to_string())?;
        let rgba = primary.capture_image().map_err(|e| format!("Screen capture returned no image: {e}"))?;
        if rgba.width() == 0 || rgba.height() == 0 {
            return Err("Screen capture returned an empty image.".into());
        }
        encode_jpeg_under_limit(DynamicImage::ImageRgba8(rgba))
    })
    .await
    .map_err(|e| format!("Screen capture task failed: {e}"))?
}

pub fn encode_jpeg_under_limit(img: DynamicImage) -> Result<CapturedScreen, String> {
    let (src_w, src_h) = img.dimensions();
    let ratio = (MAX_WIDTH as f64 / src_w as f64).min(MAX_HEIGHT as f64 / src_h as f64).min(1.0);
    let (width, height) = (
        ((src_w as f64 * ratio).round() as u32).max(1),
        ((src_h as f64 * ratio).round() as u32).max(1),
    );
    let resized = if ratio < 1.0 {
        img.resize_exact(width, height, FilterType::Triangle)
    } else {
        img
    };
    let rgb = resized.to_rgb8();

    let mut quality = JPEG_QUALITY;
    let mut jpeg = encode_jpeg(&rgb, quality)?;
    while jpeg.len() > MAX_BYTES && quality > MIN_QUALITY {
        quality = quality.saturating_sub(10).max(MIN_QUALITY);
        jpeg = encode_jpeg(&rgb, quality)?;
        if quality == MIN_QUALITY {
            break;
        }
    }

    let bytes = jpeg.len();
    let base64 = base64::engine::general_purpose::STANDARD.encode(&jpeg);
    Ok(CapturedScreen { base64, width, height, bytes })
}

fn encode_jpeg(rgb: &image::RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    let mut encoder = JpegEncoder::new_with_quality(&mut out, quality);
    encoder.encode_image(rgb).map_err(|e| format!("Could not encode screenshot JPEG: {e}"))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn resizes_oversized_image_and_caps_jpeg_size() {
        let mut img = RgbaImage::new(2560, 1600);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgba([(x % 255) as u8, (y % 255) as u8, ((x ^ y) % 255) as u8, 255]);
        }
        let shot = encode_jpeg_under_limit(DynamicImage::ImageRgba8(img)).unwrap();
        assert_eq!((shot.width, shot.height), (1280, 800));
        assert!(shot.bytes <= MAX_BYTES, "jpeg too large: {}", shot.bytes);
        assert!(!shot.base64.is_empty());
    }
}
