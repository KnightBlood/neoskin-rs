//! 皮肤图像处理：头像与预览。
//!
//! PHP 使用 `Blessing\Minecraft`（多边形 3D 渲染）+ GD；本模块实现等价的
//! **2D 视图**：头像取头部正反面 + 帽子层，预览拼接正面与背面的
//! 头/身体/手臂/腿（支持 64×64 与 64×32 皮肤、Alex 细手臂）。
//!
//! 偏差：`?3d` 目前返回 2D 头像；输出固定 PNG（PHP 默认 WebP）。
//! 这些差异记录在 `PORT-NOTES.md`。

use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

const FALLBACK_2D: &[u8] = include_bytes!("../../resources/textures/avatar2d.png");
const FALLBACK_3D: &[u8] = include_bytes!("../../resources/textures/avatar3d.png");

/// 解码 PNG（也接受其他格式，由 `image` 猜测）。
pub fn decode(data: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(data)
        .ok()
        .map(|image| image.to_rgba8())
}

pub fn fallback(three_d: bool) -> Option<RgbaImage> {
    decode(if three_d { FALLBACK_3D } else { FALLBACK_2D })
}

/// 从源图复制一块到目标画布（越界部分按透明处理）。
#[allow(clippy::too_many_arguments)]
fn blit(
    target: &mut RgbaImage,
    source: &RgbaImage,
    sx: u32,
    sy: u32,
    sw: u32,
    sh: u32,
    dx: i64,
    dy: i64,
) {
    let (width, height) = source.dimensions();
    if sx >= width || sy >= height {
        return;
    }
    let sw = sw.min(width - sx);
    let sh = sh.min(height - sy);
    for y in 0..sh {
        for x in 0..sw {
            let target_x = dx + x as i64;
            let target_y = dy + y as i64;
            if target_x < 0 || target_y < 0 {
                continue;
            }
            let (target_x, target_y) = (target_x as u32, target_y as u32);
            if target_x >= target.width() || target_y >= target.height() {
                continue;
            }
            let pixel = source.get_pixel(sx + x, sy + y);
            if pixel[3] == 0 {
                continue;
            }
            if pixel[3] == 255 {
                target.put_pixel(target_x, target_y, *pixel);
                continue;
            }
            // source-over alpha 合成
            let src = pixel.0;
            let dst = target.get_pixel(target_x, target_y).0;
            let sa = src[3] as f32 / 255.0;
            let da = dst[3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            let blend = |s: u8, d: u8| -> u8 {
                if out_a <= 0.0 {
                    return 0;
                }
                let value = (s as f32 * sa + d as f32 * da * (1.0 - sa)) / out_a;
                value.round().clamp(0.0, 255.0) as u8
            };
            target.put_pixel(
                target_x,
                target_y,
                Rgba([
                    blend(src[0], dst[0]),
                    blend(src[1], dst[1]),
                    blend(src[2], dst[2]),
                    (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
}

fn crop(source: &RgbaImage, x: u32, y: u32, width: u32, height: u32) -> RgbaImage {
    let mut output = RgbaImage::new(width, height);
    blit(&mut output, source, x, y, width, height, 0, 0);
    output
}

/// 头部（正面 + 帽子层）。
pub fn head(skin: &RgbaImage) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
    blit(&mut output, skin, 8, 8, 8, 8, 0, 0);
    blit(&mut output, skin, 40, 8, 8, 8, 0, 0);
    output
}

/// 缩放并居中为 `size × size`。
pub fn resize_square(source: &RgbaImage, size: u32) -> RgbaImage {
    let size = size.clamp(1, 2048);
    image::imageops::resize(source, size, size, FilterType::Triangle)
}

fn arm_width(slim: bool) -> u32 {
    if slim { 3 } else { 4 }
}

/// 正面视图（16×32 逻辑像素）。
pub fn front_view(skin: &RgbaImage, slim: bool) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(16, 32, Rgba([0, 0, 0, 0]));
    let arm = arm_width(slim);
    // 腿
    blit(&mut output, skin, 4, 20, 4, 12, 4, 20);
    blit(&mut output, skin, 20, 52, 4, 12, 8, 20);
    blit(&mut output, skin, 4, 36, 4, 12, 4, 20);
    blit(&mut output, skin, 4, 52, 4, 12, 8, 20);
    // 身体
    blit(&mut output, skin, 20, 20, 8, 12, 4, 8);
    blit(&mut output, skin, 20, 36, 8, 12, 4, 8);
    // 手臂
    blit(&mut output, skin, 44, 20, arm, 12, 0, 8);
    blit(&mut output, skin, 44, 36, arm, 12, 0, 8);
    let left_x = if slim { 13 } else { 12 };
    blit(&mut output, skin, 36, 52, arm, 12, left_x, 8);
    blit(&mut output, skin, 52, 52, arm, 12, left_x, 8);
    // 头
    blit(&mut output, skin, 8, 8, 8, 8, 4, 0);
    blit(&mut output, skin, 40, 8, 8, 8, 4, 0);
    output
}

/// 背面视图（16×32 逻辑像素）。
pub fn back_view(skin: &RgbaImage, slim: bool) -> RgbaImage {
    let mut output = RgbaImage::from_pixel(16, 32, Rgba([0, 0, 0, 0]));
    let arm = arm_width(slim);
    // 腿（背面左右互换）
    blit(&mut output, skin, 12, 20, 4, 12, 8, 20);
    blit(&mut output, skin, 28, 52, 4, 12, 4, 20);
    blit(&mut output, skin, 12, 36, 4, 12, 8, 20);
    blit(&mut output, skin, 12, 52, 4, 12, 4, 20);
    // 身体
    blit(&mut output, skin, 32, 20, 8, 12, 4, 8);
    blit(&mut output, skin, 32, 36, 8, 12, 4, 8);
    // 手臂
    blit(&mut output, skin, 52, 20, arm, 12, 12, 8);
    blit(&mut output, skin, 52, 36, arm, 12, 12, 8);
    blit(&mut output, skin, 44, 52, arm, 12, 0, 8);
    blit(&mut output, skin, 60, 52, arm, 12, 0, 8);
    // 头
    blit(&mut output, skin, 24, 8, 8, 8, 4, 0);
    blit(&mut output, skin, 56, 8, 8, 8, 4, 0);
    output
}

/// 皮肤预览：正面与背面并排，每逻辑像素 `ratio` 像素。
pub fn skin_preview(skin: &RgbaImage, ratio: u32, slim: bool) -> RgbaImage {
    let ratio = ratio.clamp(1, 40);
    let front = image::imageops::resize(
        &front_view(skin, slim),
        16 * ratio,
        32 * ratio,
        FilterType::Nearest,
    );
    let back = image::imageops::resize(
        &back_view(skin, slim),
        16 * ratio,
        32 * ratio,
        FilterType::Nearest,
    );
    let pad_x = 15 * ratio / 12;
    let pad_y = 30 * ratio / 12;
    let gap = 15 * ratio / 12;
    let width = pad_x * 2 + front.width() * 2 + gap;
    let height = pad_y * 2 + front.height();
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    image::imageops::overlay(&mut canvas, &back, pad_x as i64, pad_y as i64);
    image::imageops::overlay(
        &mut canvas,
        &front,
        (pad_x + front.width() + gap) as i64,
        pad_y as i64,
    );
    canvas
}

/// 披风预览：取披风正面 10×16 区域，按高度缩放。
pub fn cape_preview(cape: &RgbaImage, height: u32) -> RgbaImage {
    let height = height.clamp(1, 2048);
    let front = crop(cape, 1, 1, 10, 16);
    let width = (height as f64 / 16.0 * 10.0).round().max(1.0) as u32;
    let scaled = image::imageops::resize(&front, width, height, FilterType::Triangle);
    let pad_x = 40u32;
    let pad_y = 20u32;
    let mut canvas = RgbaImage::from_pixel(
        pad_x * 2 + scaled.width(),
        pad_y * 2 + scaled.height(),
        Rgba([0, 0, 0, 0]),
    );
    image::imageops::overlay(&mut canvas, &scaled, pad_x as i64, pad_y as i64);
    canvas
}

pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut std::io::Cursor::new(&mut output), ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank_skin() -> RgbaImage {
        let mut skin = RgbaImage::from_pixel(64, 64, Rgba([0, 0, 0, 0]));
        for y in 0..8 {
            for x in 0..8 {
                skin.put_pixel(8 + x, 8 + y, Rgba([200, 40, 40, 255]));
            }
        }
        skin
    }

    #[test]
    fn head_extracts_face_region() {
        let skin = blank_skin();
        let head = head(&skin);
        assert_eq!(head.dimensions(), (8, 8));
        assert_eq!(head.get_pixel(0, 0)[0], 200);
    }

    #[test]
    fn preview_has_expected_ratio() {
        let skin = blank_skin();
        let preview = skin_preview(&skin, 12, false);
        assert!(preview.width() > 300);
        assert!(preview.height() > 300);
    }

    #[test]
    fn fallback_images_decode() {
        assert!(fallback(false).is_some());
        assert!(fallback(true).is_some());
    }

    #[test]
    fn encodes_png_roundtrip() {
        let bytes = encode_png(&blank_skin()).expect("png");
        let decoded = image::load_from_memory(&bytes).expect("decode");
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 64);
    }
}
