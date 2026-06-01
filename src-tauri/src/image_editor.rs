use image::{DynamicImage, ImageEncoder, ImageFormat};
use imageproc::geometric_transformations::{rotate_about_center, Interpolation};
use serde::Serialize;
use std::path::Path;

// ─── Types ────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct ImageInfo {
    pub path: String,
    pub filename: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Clone)]
pub struct ImageOpResult {
    pub output_path: String,
    pub output_size_bytes: u64,
    pub width: u32,
    pub height: u32,
}

// ─── Helpers ──────────────────────────────────────────────────

fn image_format_from_ext(path: &str) -> ImageFormat {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => ImageFormat::Png,
        "webp" => ImageFormat::WebP,
        "bmp" => ImageFormat::Bmp,
        "tiff" | "tif" => ImageFormat::Tiff,
        "gif" => ImageFormat::Gif,
        _ => ImageFormat::Jpeg,
    }
}

fn output_path_for(input: &str, suffix: &str, ext: &str) -> String {
    let p = Path::new(input);
    let stem = p
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let parent = p
        .parent()
        .and_then(|d| d.to_str())
        .unwrap_or(".");
    format!("{}/{}{}.{}", parent, stem, suffix, ext)
}

fn image_format_label(img: &DynamicImage, ext: &str) -> String {
    let c = img.color();
    let depth = match c.bytes_per_pixel() {
        1 => "8-bit gray",
        3 => "24-bit RGB",
        4 => "32-bit RGBA",
        _ => "unknown",
    };
    format!("{} ({})", ext.to_uppercase(), depth)
}

fn save_image(img: &DynamicImage, path: &str) -> Result<(u32, u32, u64), String> {
    let w = img.width();
    let h = img.height();
    let format = image_format_from_ext(path);

    let mut buf = std::io::Cursor::new(Vec::new());
    match format {
        ImageFormat::Jpeg => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85);
            encoder
                .write_image(
                    img.as_bytes(),
                    w,
                    h,
                    img.color().into(),
                )
                .map_err(|e| format!("JPEG 编码失败: {}", e))?;
        }
        ImageFormat::Png => {
            img.write_to(&mut buf, ImageFormat::Png)
                .map_err(|e| format!("PNG 编码失败: {}", e))?;
        }
        ImageFormat::WebP => {
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            encoder
                .write_image(
                    img.as_bytes(),
                    w,
                    h,
                    img.color().into(),
                )
                .map_err(|e| format!("WebP 编码失败: {}", e))?;
        }
        ImageFormat::Bmp => {
            img.write_to(&mut buf, ImageFormat::Bmp)
                .map_err(|e| format!("BMP 编码失败: {}", e))?;
        }
        ImageFormat::Tiff => {
            img.write_to(&mut buf, ImageFormat::Tiff)
                .map_err(|e| format!("TIFF 编码失败: {}", e))?;
        }
        _ => {
            img.write_to(&mut buf, ImageFormat::Png)
                .map_err(|e| format!("编码失败: {}", e))?;
        }
    }

    let bytes = buf.into_inner();
    std::fs::write(path, &bytes).map_err(|e| format!("写入文件失败: {}", e))?;
    Ok((w, h, bytes.len() as u64))
}

fn load_image(path: &str) -> Result<DynamicImage, String> {
    image::open(path).map_err(|e| format!("无法打开图片: {}", e))
}

// ─── Commands ─────────────────────────────────────────────────

#[tauri::command]
pub async fn image_open_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title("选择图片文件")
        .add_filter("图片文件", &["png", "jpg", "jpeg", "webp", "bmp", "tiff", "tif", "gif"])
        .add_filter("所有文件", &["*"])
        .pick_file()
        .await
        .map(|h| h.path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn image_get_info(path: String) -> Result<ImageInfo, String> {
    let img = load_image(&path)?;
    let meta = std::fs::metadata(&path)
        .map_err(|e| format!("读取文件信息失败: {}", e))?;
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    Ok(ImageInfo {
        path: path.clone(),
        filename: Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        width: img.width(),
        height: img.height(),
        format: image_format_label(&img, &ext),
        size_bytes: meta.len(),
    })
}

/// Save a copy of the image (used to avoid overwriting in-place)
#[tauri::command]
pub async fn image_save_copy(path: String, suffix: String) -> Result<String, String> {
    let img = load_image(&path)?;
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let out = output_path_for(&path, &suffix, &ext);
    save_image(&img, &out)?;
    Ok(out)
}

/// Crop: x, y, width, height in original image pixels
#[tauri::command]
pub async fn image_crop(
    path: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<ImageOpResult, String> {
    let mut img = load_image(&path)?;
    let cropped = img.crop(x, y, width, height);
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let out = output_path_for(&path, "_cropped", ext);
    let (w, h, s) = save_image(&cropped, &out)?;
    Ok(ImageOpResult { output_path: out, output_size_bytes: s, width: w, height: h })
}

/// Rotate: degrees - supports arbitrary angles (will expand canvas to fit)
#[tauri::command]
pub async fn image_rotate(
    path: String,
    degrees: f32,
) -> Result<ImageOpResult, String> {
    let img = load_image(&path)?;
    let rotated = rotate_about_center(
        &img.to_rgba8(),
        std::f32::consts::PI * degrees / 180.0,
        Interpolation::Bilinear,
        image::Rgba([0, 0, 0, 0]),
    );
    let dyn_img = DynamicImage::ImageRgba8(rotated);
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let suffix = format!("_rotated_{}", degrees as i32);
    let out = output_path_for(&path, &suffix, ext);
    let (w, h, s) = save_image(&dyn_img, &out)?;
    Ok(ImageOpResult { output_path: out, output_size_bytes: s, width: w, height: h })
}

/// Resize: target width / height; set one to 0 to auto-calc keeping ratio
#[tauri::command]
pub async fn image_resize(
    path: String,
    target_width: u32,
    target_height: u32,
    keep_ratio: bool,
) -> Result<ImageOpResult, String> {
    let img = load_image(&path)?;
    let (iw, ih) = (img.width(), img.height());

    let (nw, nh) = if keep_ratio {
        if target_width > 0 && target_height > 0 {
            // Fit within box
            let ratio = (target_width as f64 / iw as f64)
                .min(target_height as f64 / ih as f64);
            ((iw as f64 * ratio) as u32, (ih as f64 * ratio) as u32)
        } else if target_width > 0 {
            let ratio = target_width as f64 / iw as f64;
            (target_width, (ih as f64 * ratio) as u32)
        } else if target_height > 0 {
            let ratio = target_height as f64 / ih as f64;
            ((iw as f64 * ratio) as u32, target_height)
        } else {
            (iw, ih)
        }
    } else {
        let w = if target_width > 0 { target_width } else { iw };
        let h = if target_height > 0 { target_height } else { ih };
        (w, h)
    };

    let resized = img.resize_exact(nw.max(1), nh.max(1), image::imageops::FilterType::Lanczos3);
    let ext = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let suffix = format!("_{}x{}", nw, nh);
    let out = output_path_for(&path, &suffix, ext);
    let (w, h, s) = save_image(&resized, &out)?;
    Ok(ImageOpResult { output_path: out, output_size_bytes: s, width: w, height: h })
}

/// Compress: adjust JPEG quality (1-100) or convert PNG→JPEG/WebP for size reduction
#[tauri::command]
pub async fn image_compress(
    path: String,
    quality: u8,
    format: Option<String>,
) -> Result<ImageOpResult, String> {
    let img = load_image(&path)?;
    let target_ext = format
        .unwrap_or_else(|| {
            let e = Path::new(&path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("jpg")
                .to_lowercase();
            if e == "png" || e == "bmp" || e == "tiff" || e == "tif" {
                "jpg".to_string()
            } else {
                e
            }
        });
    let suffix = format!("_q{}", quality);
    let out = output_path_for(&path, &suffix, &target_ext);
    let w = img.width();
    let h = img.height();

    let mut buf = std::io::Cursor::new(Vec::new());
    match target_ext.as_str() {
        "webp" => {
            let encoder = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
            encoder
                .write_image(img.as_bytes(), w, h, img.color().into())
                .map_err(|e| format!("WebP 编码失败: {}", e))?;
        }
        "png" => {
            // PNG compression via write_to
            img.write_to(&mut buf, ImageFormat::Png)
                .map_err(|e| format!("PNG 编码失败: {}", e))?;
        }
        _ => {
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
            encoder
                .write_image(img.as_bytes(), w, h, img.color().into())
                .map_err(|e| format!("JPEG 编码失败: {}", e))?;
        }
    }

    let bytes = buf.into_inner();
    std::fs::write(&out, &bytes).map_err(|e| format!("写入文件失败: {}", e))?;
    Ok(ImageOpResult {
        output_path: out,
        output_size_bytes: bytes.len() as u64,
        width: w,
        height: h,
    })
}

/// Convert format: change image to target format
#[tauri::command]
pub async fn image_convert(
    path: String,
    output_format: String,
) -> Result<ImageOpResult, String> {
    let img = load_image(&path)?;
    let out = output_path_for(&path, "_converted", &output_format);
    let (w, h, s) = save_image(&img, &out)?;
    Ok(ImageOpResult { output_path: out, output_size_bytes: s, width: w, height: h })
}

/// 弹出另存为对话框，将 src 文件复制到用户指定位置
#[tauri::command]
pub async fn file_save_as(src: String, default_name: String) -> Result<Option<String>, String> {
    let dest = rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .save_file()
        .await;
    if let Some(handle) = dest {
        let dest_path = handle.path().to_path_buf();
        std::fs::copy(&src, &dest_path)
            .map_err(|e| format!("保存失败: {}", e))?;
        Ok(Some(dest_path.to_string_lossy().to_string()))
    } else {
        Ok(None)
    }
}

/// 将 src 文件覆盖写入 dst（保存并覆盖原文件）
#[tauri::command]
pub fn file_overwrite(src: String, dst: String) -> Result<(), String> {
    std::fs::copy(&src, &dst)
        .map_err(|e| format!("覆盖原文件失败: {}", e))?;
    Ok(())
}