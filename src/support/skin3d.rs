//! 3D 头像渲染：`Blessing\Renderer\SkinRenderer` 的移植。
//!
//! 与 `skin.rs` 的 2D 视图不同，这里实现 PHP `render3dAvatar()` 使用的
//! 等距投影 + 多边形扫描线填充。算法逐段对齐 Go 端口（`internal/minecraft`），
//! 包括上游的细节（例如部分 `has()` 守卫与写入键不一致）。
//!
//! 画布内部使用 `0xAARRGGBB`，与 GD 的 `gdAlpha` 互转。

use std::collections::HashMap;

use image::RgbaImage;

const ALL_FACES: [&str; 6] = ["back", "right", "top", "front", "left", "bottom"];
const MEMBERS: [&str; 6] = [
    "head", "torso", "rightArm", "leftArm", "rightLeg", "leftLeg",
];
const PIECES: [&str; 7] = [
    "helmet", "head", "torso", "rightArm", "leftArm", "rightLeg", "leftLeg",
];

// ---------------------------------------------------------------- canvas

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    pixels: Vec<u32>,
}

impl Canvas {
    fn new(width: i32, height: i32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            pixels: vec![0x00FF_FFFF; (width * height) as usize],
        }
    }

    fn from_rgba(image: &RgbaImage) -> Self {
        let (width, height) = image.dimensions();
        let mut canvas = Self::new(width as i32, height as i32);
        for (x, y, pixel) in image.enumerate_pixels() {
            let [r, g, b, a] = pixel.0;
            canvas.pixels[(y * width + x) as usize] =
                (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
        canvas
    }

    fn to_rgba(&self) -> RgbaImage {
        let mut output = RgbaImage::new(self.width as u32, self.height as u32);
        for y in 0..self.height {
            for x in 0..self.width {
                let argb = self.pixels[(y * self.width + x) as usize];
                output.put_pixel(
                    x as u32,
                    y as u32,
                    image::Rgba([
                        ((argb >> 16) & 0xFF) as u8,
                        ((argb >> 8) & 0xFF) as u8,
                        (argb & 0xFF) as u8,
                        ((argb >> 24) & 0xFF) as u8,
                    ]),
                );
            }
        }
        output
    }

    fn pixel_at(&self, x: i32, y: i32) -> u32 {
        let x = x.clamp(0, self.width - 1);
        let y = y.clamp(0, self.height - 1);
        self.pixels[(y * self.width + x) as usize]
    }

    fn set_pixel(&mut self, x: i32, y: i32, argb: u32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        self.pixels[(y * self.width + x) as usize] = argb;
    }

    fn blend_pixel(&mut self, x: i32, y: i32, argb: u32) {
        let alpha = (argb >> 24) as i32;
        if alpha <= 0 {
            return;
        }
        if alpha == 255 {
            self.set_pixel(x, y, argb);
            return;
        }
        self.blend(x, y, argb, alpha);
    }

    fn blend(&mut self, x: i32, y: i32, argb: u32, alpha: i32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let index = (y * self.width + x) as usize;
        let dst = self.pixels[index];
        let sa = alpha as f64 / 255.0;
        let da = ((dst >> 24) & 0xFF) as f64 / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            self.pixels[index] = 0;
            return;
        }
        let channel = |shift: u32| -> u32 {
            let s = ((argb >> shift) & 0xFF) as f64 / 255.0;
            let d = ((dst >> shift) & 0xFF) as f64 / 255.0;
            (((s * sa + d * da * (1.0 - sa)) / out_a) * 255.0).round() as u32
        };
        self.pixels[index] = ((out_a * 255.0).round() as u32) << 24
            | channel(16) << 16
            | channel(8) << 8
            | channel(0);
    }

    fn color_at_gd(&self, x: i32, y: i32) -> u32 {
        let argb = self.pixel_at(x, y);
        let gd_alpha = 127 - (((argb >> 24) * 127) as f64 / 255.0).round() as u32;
        gd_alpha << 24 | (argb & 0x00FF_FFFF)
    }

    fn filled_rectangle(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, gd_color: u32) {
        let alpha = alpha255(gd_color);
        if alpha <= 0 {
            return;
        }
        let left = x1.min(x2);
        let right = x1.max(x2);
        let top = y1.min(y2);
        let bottom = y1.max(y2);
        let argb = to_argb(gd_color);
        for y in top.max(0)..=bottom.min(self.height - 1) {
            for x in left.max(0)..=right.min(self.width - 1) {
                self.blend(x, y, argb, alpha);
            }
        }
    }

    fn filled_polygon(&mut self, points: &[f64], gd_color: u32) {
        let alpha = alpha255(gd_color);
        let count = points.len() / 2;
        if alpha <= 0 || count < 3 {
            return;
        }
        let mut xs = vec![0i32; count];
        let mut ys = vec![0i32; count];
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for i in 0..count {
            xs[i] = points[i * 2].round() as i32;
            ys[i] = points[i * 2 + 1].round() as i32;
            min_y = min_y.min(ys[i]);
            max_y = max_y.max(ys[i]);
        }
        let argb = to_argb(gd_color);
        for y in min_y.max(0)..=max_y.min(self.height - 1) {
            let mut crossings: Vec<f64> = Vec::with_capacity(count);
            for i in 0..count {
                let j = (i + 1) % count;
                let (y1, y2) = (ys[i], ys[j]);
                if y1 == y2 {
                    continue;
                }
                let scan = y as f64 + 0.5;
                if (y1 as f64 <= scan && y2 as f64 > scan)
                    || (y2 as f64 <= scan && y1 as f64 > scan)
                {
                    let x = xs[i] as f64
                        + (scan - y1 as f64) * (xs[j] - xs[i]) as f64 / (y2 - y1) as f64;
                    crossings.push(x);
                }
            }
            crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mut k = 0;
            while k + 1 < crossings.len() {
                let from = (crossings[k].ceil() as i32).max(0);
                let to = (crossings[k + 1].floor() as i32).min(self.width - 1);
                for x in from..=to {
                    self.blend(x, y, argb, alpha);
                }
                k += 2;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn resample_region(
        &mut self,
        source: &Canvas,
        dst_x: i32,
        dst_y: i32,
        dst_w: i32,
        dst_h: i32,
        src_x: i32,
        src_y: i32,
        src_w: i32,
        src_h: i32,
    ) {
        if dst_w <= 0 || dst_h <= 0 || src_w <= 0 || src_h <= 0 {
            return;
        }
        for y in 0..dst_h {
            let sy = src_y as f64 + (y as f64 + 0.5) * src_h as f64 / dst_h as f64;
            let y0 = ((sy - 0.5).floor() as i32).clamp(0, source.height - 1);
            let y1 = (y0 + 1).min(source.height - 1);
            let fy = (sy - 0.5 - y0 as f64).clamp(0.0, 1.0);
            for x in 0..dst_w {
                let sx = src_x as f64 + (x as f64 + 0.5) * src_w as f64 / dst_w as f64;
                let x0 = ((sx - 0.5).floor() as i32).clamp(0, source.width - 1);
                let x1 = (x0 + 1).min(source.width - 1);
                let fx = (sx - 0.5 - x0 as f64).clamp(0.0, 1.0);
                let argb = bilinear(
                    source.pixel_at(x0, y0),
                    source.pixel_at(x1, y0),
                    source.pixel_at(x0, y1),
                    source.pixel_at(x1, y1),
                    fx,
                    fy,
                );
                self.set_pixel(dst_x + x, dst_y + y, argb);
            }
        }
    }
}

fn bilinear(p00: u32, p10: u32, p01: u32, p11: u32, fx: f64, fy: f64) -> u32 {
    let channel = |shift: u32| -> u32 {
        let c00 = ((p00 >> shift) & 0xFF) as f64;
        let c10 = ((p10 >> shift) & 0xFF) as f64;
        let c01 = ((p01 >> shift) & 0xFF) as f64;
        let c11 = ((p11 >> shift) & 0xFF) as f64;
        let top = c00 + (c10 - c00) * fx;
        let bottom = c01 + (c11 - c01) * fx;
        (top + (bottom - top) * fy).round() as u32
    };
    channel(24) << 24 | channel(16) << 16 | channel(8) << 8 | channel(0)
}

fn alpha255(gd_color: u32) -> i32 {
    let gd_alpha = ((gd_color >> 24) & 0x7F) as f64;
    ((127.0 - gd_alpha) * 255.0 / 127.0).round() as i32
}

fn to_argb(gd_color: u32) -> u32 {
    (alpha255(gd_color) as u32) << 24 | (gd_color & 0x00FF_FFFF)
}

// ---------------------------------------------------------------- geometry

#[derive(Clone, Copy, Default)]
pub struct Point3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub dest_x: f64,
    pub dest_y: f64,
    pub dest_z: f64,
    pub projected: bool,
    pre_projected: bool,
}

impl Point3 {
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            x,
            y,
            z,
            ..Default::default()
        }
    }

    fn project(&mut self, cos_a: f64, sin_a: f64, cos_o: f64, sin_o: f64, bounds: &mut Bounds) {
        let yy = self.y;
        let zz = self.z;
        self.dest_x = self.x * cos_o + zz * sin_o;
        self.dest_y = self.x * sin_a * sin_o + yy * cos_a - zz * sin_a * cos_o;
        self.dest_z = -self.x * cos_a * sin_o + yy * sin_a + zz * cos_a * cos_o;
        self.projected = true;
        bounds.min_x = bounds.min_x.min(self.dest_x);
        bounds.max_x = bounds.max_x.max(self.dest_x);
        bounds.min_y = bounds.min_y.min(self.dest_y);
        bounds.max_y = bounds.max_y.max(self.dest_y);
    }

    #[allow(clippy::too_many_arguments)]
    fn pre_project(
        &mut self,
        dx: f64,
        dy: f64,
        dz: f64,
        cos_a: f64,
        sin_a: f64,
        cos_o: f64,
        sin_o: f64,
    ) {
        if self.pre_projected {
            return;
        }
        let (lx, ly, lz) = (self.x - dx, self.y - dy, self.z - dz);
        self.x = lx * cos_o + lz * sin_o + dx;
        self.y = lx * sin_a * sin_o + ly * cos_a - lz * sin_a * cos_o + dy;
        self.z = -lx * cos_a * sin_o + ly * sin_a + lz * cos_a * cos_o + dz;
        self.pre_projected = true;
    }
}

#[derive(Clone, Copy, Default)]
struct Bounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
}

struct Polygon {
    dots: Vec<Point3>,
    color: u32,
    projected: bool,
}

impl Polygon {
    fn new(dots: Vec<Point3>, color: u32) -> Self {
        Self {
            dots,
            color,
            projected: false,
        }
    }

    fn draw(&self, image: &mut Canvas, min_x: f64, min_y: f64, ratio: f64) {
        if alpha255(self.color) <= 0 || self.dots.is_empty() {
            return;
        }
        let first = &self.dots[0];
        let same_plan_x = self.dots.iter().all(|dot| dot.dest_x == first.dest_x);
        let same_plan_y = self.dots.iter().all(|dot| dot.dest_y == first.dest_y);
        if same_plan_x || same_plan_y {
            return;
        }
        let mut points = Vec::with_capacity(self.dots.len() * 2);
        for dot in &self.dots {
            points.push((dot.dest_x - min_x) * ratio);
            points.push((dot.dest_y - min_y) * ratio);
        }
        image.filled_polygon(&points, self.color);
    }

    fn project(&mut self, cos_a: f64, sin_a: f64, cos_o: f64, sin_o: f64, bounds: &mut Bounds) {
        for dot in &mut self.dots {
            if !dot.projected {
                dot.project(cos_a, sin_a, cos_o, sin_o, bounds);
            }
        }
        self.projected = true;
    }

    #[allow(clippy::too_many_arguments)]
    fn pre_project(
        &mut self,
        dx: f64,
        dy: f64,
        dz: f64,
        cos_a: f64,
        sin_a: f64,
        cos_o: f64,
        sin_o: f64,
    ) {
        for dot in &mut self.dots {
            dot.pre_project(dx, dy, dz, cos_a, sin_a, cos_o, sin_o);
        }
    }
}

#[derive(Clone, Copy)]
struct CubePoint {
    point: Point3,
    faces: [&'static str; 3],
}

#[derive(Clone)]
struct FacePair {
    front: Vec<&'static str>,
}

// ---------------------------------------------------------------- renderer

pub struct SkinRenderer {
    ratio: f64,
    head_only: bool,
    layers: bool,

    h_rotation: f64,
    v_rotation: f64,
    head_rotation: f64,
    left_leg_rotation: f64,
    right_leg_rotation: f64,
    left_arm_rotation: f64,
    right_arm_rotation: f64,

    skin: Option<Canvas>,
    new_skin: bool,
    alex: bool,
    hd: f64,

    alpha: f64,
    omega: f64,
    cos_alpha: f64,
    sin_alpha: f64,
    cos_omega: f64,
    sin_omega: f64,

    member_angles: HashMap<&'static str, Angles>,
    bounds: Bounds,

    front_faces: Vec<&'static str>,
    back_faces: Vec<&'static str>,

    visible_faces: HashMap<&'static str, FacePair>,
    cube_points: Vec<CubePoint>,

    polygons: HashMap<&'static str, HashMap<&'static str, Vec<Polygon>>>,
    volume_points: HashMap<(i64, i64, i64), Point3>,
}

#[derive(Clone, Copy)]
struct Angles {
    cos_alpha: f64,
    sin_alpha: f64,
    cos_omega: f64,
    sin_omega: f64,
}

impl Angles {
    fn new(alpha_rad: f64, omega_rad: f64) -> Self {
        Self {
            cos_alpha: alpha_rad.cos(),
            sin_alpha: alpha_rad.sin(),
            cos_omega: omega_rad.cos(),
            sin_omega: omega_rad.sin(),
        }
    }
}

impl SkinRenderer {
    fn new(ratio: f64, head_only: bool, h_rotation: f64) -> Self {
        Self {
            ratio,
            head_only,
            layers: true,
            h_rotation,
            v_rotation: -25.0,
            head_rotation: 0.0,
            left_leg_rotation: 0.0,
            right_leg_rotation: 0.0,
            left_arm_rotation: 0.0,
            right_arm_rotation: 0.0,
            skin: None,
            new_skin: false,
            alex: false,
            hd: 1.0,
            alpha: 0.0,
            omega: 0.0,
            cos_alpha: 1.0,
            sin_alpha: 0.0,
            cos_omega: 1.0,
            sin_omega: 0.0,
            member_angles: HashMap::new(),
            bounds: Bounds::default(),
            front_faces: Vec::new(),
            back_faces: Vec::new(),
            visible_faces: HashMap::new(),
            cube_points: Vec::new(),
            polygons: HashMap::new(),
            volume_points: HashMap::new(),
        }
    }

    fn render(mut self, source: &RgbaImage, alex: bool) -> Canvas {
        let mut skin = Canvas::from_rgba(source);
        if skin.width > 256 {
            let new_height = (skin.height as f64 / (skin.width as f64 / 256.0)) as i32;
            let mut dest = Canvas::new(256, new_height);
            dest.resample_region(&skin, 0, 0, 256, new_height, 0, 0, skin.width, skin.height);
            skin = dest;
        }
        self.hd = skin.width as f64 / 64.0;
        self.new_skin = skin.width == skin.height;
        self.alex = alex;
        self.skin = Some(skin);

        self.make_background_transparent();
        if self.layers {
            self.fix_new_skin_type_layers();
        }
        self.calculate_angles();
        self.faces_determination();
        self.generate_polygons();
        self.member_rotation();
        self.create_projection_plan();
        let image = self.display_image();
        self.skin = None;
        image
    }

    fn make_background_transparent(&mut self) {
        let skin = self.skin.as_ref().expect("skin");
        let mut temp_value = i64::MIN;
        let mut need_remove = true;

        for i_h in 0..8 {
            for i_v in 0..8 {
                let pixel_color = skin.color_at_gd(i_h, i_v);
                let gd_alpha = ((pixel_color >> 24) & 0x7F) as i32;
                if gd_alpha > 120 {
                    need_remove = false;
                }
                if temp_value == i64::MIN {
                    temp_value = pixel_color as i64;
                } else if temp_value != pixel_color as i64 {
                    need_remove = false;
                }
            }
        }

        let (img_x, img_y) = (skin.width, skin.height);
        let mut dst = Canvas::new(img_x, img_y);
        let bg_rgb = if need_remove {
            (temp_value as u32) & 0x00FF_FFFF
        } else {
            0
        };
        let fill_gd = bg_rgb;

        let position_multiply = img_x as f64 / 64.0;
        let mut rect_f = |x1: i32, y1: i32, x2: i32, y2: i32| {
            dst.filled_rectangle(
                (x1 as f64 * position_multiply) as i32,
                (y1 as f64 * position_multiply) as i32,
                (x2 as f64 * position_multiply) as i32,
                (y2 as f64 * position_multiply) as i32,
                fill_gd,
            );
        };
        rect_f(8, 0, 23, 7);
        rect_f(0, 8, 31, 15);
        rect_f(4, 16, 11, 19);
        rect_f(20, 16, 35, 19);
        rect_f(44, 16, 51, 19);
        rect_f(0, 20, 54, 31);
        rect_f(20, 48, 27, 51);
        rect_f(36, 48, 43, 51);
        rect_f(16, 52, 47, 63);

        let skin = self.skin.as_ref().expect("skin");
        for y in 0..img_y {
            for x in 0..img_x {
                let src = skin.pixel_at(x, y);
                if need_remove && (src & 0x00FF_FFFF) == bg_rgb {
                    continue;
                }
                let src_alpha = ((src >> 24) & 0xFF) as f32;
                let src_gd_alpha = 127 - java_round(src_alpha * 127.0 / 255.0);
                if src_gd_alpha >= 127 {
                    continue;
                }
                dst.blend_pixel(x, y, src);
            }
        }
        self.skin = Some(dst);
    }

    fn fix_new_skin_type_layers(&mut self) {
        if !self.new_skin {
            return;
        }
        let skin = self.skin.as_mut().expect("skin");
        copy_self(skin, 0, 16, 0, 32, 56, 16);
        copy_self(skin, 16, 48, 0, 48, 16, 16);
        copy_self(skin, 32, 48, 48, 48, 16, 16);
    }

    fn calculate_angles(&mut self) {
        self.alpha = self.v_rotation * std::f64::consts::PI / 180.0;
        self.omega = self.h_rotation * std::f64::consts::PI / 180.0;
        self.cos_alpha = self.alpha.cos();
        self.sin_alpha = self.alpha.sin();
        self.cos_omega = self.omega.cos();
        self.sin_omega = self.omega.sin();

        self.member_angles.insert("torso", Angles::new(0.0, 0.0));
        let head = Angles::new(0.0, self.head_rotation * std::f64::consts::PI / 180.0);
        self.member_angles.insert("head", head);
        self.member_angles.insert("helmet", head);
        self.member_angles.insert(
            "rightArm",
            Angles::new(self.right_arm_rotation * std::f64::consts::PI / 180.0, 0.0),
        );
        self.member_angles.insert(
            "leftArm",
            Angles::new(self.left_arm_rotation * std::f64::consts::PI / 180.0, 0.0),
        );
        self.member_angles.insert(
            "rightLeg",
            Angles::new(self.right_leg_rotation * std::f64::consts::PI / 180.0, 0.0),
        );
        self.member_angles.insert(
            "leftLeg",
            Angles::new(self.left_leg_rotation * std::f64::consts::PI / 180.0, 0.0),
        );
        self.bounds = Bounds::default();
    }

    fn faces_determination(&mut self) {
        let (ca, sa, co, so) = (
            self.cos_alpha,
            self.sin_alpha,
            self.cos_omega,
            self.sin_omega,
        );
        self.visible_faces.clear();
        for part in MEMBERS {
            self.visible_faces
                .insert(part, FacePair { front: Vec::new() });
        }

        for part in MEMBERS {
            self.set_cube_points();
            let angles = self.member_angles[part];
            let mut points = std::mem::take(&mut self.cube_points);
            let mut cube_max_depth: Option<CubePoint> = None;
            for item in &mut points {
                item.point.pre_project(
                    0.0,
                    0.0,
                    0.0,
                    angles.cos_alpha,
                    angles.sin_alpha,
                    angles.cos_omega,
                    angles.sin_omega,
                );
                item.point.project(ca, sa, co, so, &mut self.bounds);
                let replace = match cube_max_depth {
                    None => true,
                    Some(current) => current.point.dest_z > item.point.dest_z,
                };
                if replace {
                    cube_max_depth = Some(*item);
                }
            }
            self.cube_points = points;
            let back: Vec<&'static str> = cube_max_depth
                .map(|item| item.faces.to_vec())
                .unwrap_or_default();
            let front = array_diff(&ALL_FACES, &back);
            self.visible_faces.insert(part, FacePair { front });
        }

        self.set_cube_points();
        let mut points = std::mem::take(&mut self.cube_points);
        let mut max_depth: Option<CubePoint> = None;
        for item in &mut points {
            item.point.project(ca, sa, co, so, &mut self.bounds);
            let replace = match max_depth {
                None => true,
                Some(current) => current.point.dest_z > item.point.dest_z,
            };
            if replace {
                max_depth = Some(*item);
            }
        }
        self.cube_points = points;
        self.back_faces = max_depth
            .map(|item| item.faces.to_vec())
            .unwrap_or_default();
        self.front_faces = array_diff(&ALL_FACES, &self.back_faces);
    }

    fn set_cube_points(&mut self) {
        self.cube_points = vec![
            CubePoint {
                point: Point3::new(0.0, 0.0, 0.0),
                faces: ["back", "right", "top"],
            },
            CubePoint {
                point: Point3::new(0.0, 0.0, 1.0),
                faces: ["front", "right", "top"],
            },
            CubePoint {
                point: Point3::new(0.0, 1.0, 0.0),
                faces: ["back", "right", "bottom"],
            },
            CubePoint {
                point: Point3::new(0.0, 1.0, 1.0),
                faces: ["front", "right", "bottom"],
            },
            CubePoint {
                point: Point3::new(1.0, 0.0, 0.0),
                faces: ["back", "left", "top"],
            },
            CubePoint {
                point: Point3::new(1.0, 0.0, 1.0),
                faces: ["front", "left", "top"],
            },
            CubePoint {
                point: Point3::new(1.0, 1.0, 0.0),
                faces: ["back", "left", "bottom"],
            },
            CubePoint {
                point: Point3::new(1.0, 1.0, 1.0),
                faces: ["front", "left", "bottom"],
            },
        ];
    }

    fn generate_polygons(&mut self) {
        self.polygons.clear();
        for part in PIECES {
            let mut faces: HashMap<&'static str, Vec<Polygon>> = HashMap::new();
            for face in ALL_FACES {
                faces.insert(face, Vec::new());
            }
            self.polygons.insert(part, faces);
        }
        self.head();
        self.helmet();
        if !self.head_only {
            self.torso();
            self.right_arm();
            self.left_arm();
            self.right_leg();
            self.left_leg();
        }
    }

    fn has(&self, i: f64, j: f64, k: f64) -> bool {
        self.volume_points
            .contains_key(&(i as i64, j as i64, k as i64))
    }

    fn put(&mut self, i: f64, j: f64, k: f64, x: f64, y: f64, z: f64) {
        self.volume_points
            .insert((i as i64, j as i64, k as i64), Point3::new(x, y, z));
    }

    fn pt(&self, i: f64, j: f64, k: f64) -> Point3 {
        *self
            .volume_points
            .get(&(i as i64, j as i64, k as i64))
            .unwrap_or_else(|| panic!("volume point missing: {i},{j},{k}"))
    }

    fn color(&self, x: i32, y: i32) -> u32 {
        self.skin.as_ref().expect("skin").color_at_gd(x, y)
    }

    #[allow(clippy::too_many_arguments)]
    fn quad(
        &mut self,
        part: &'static str,
        face: &'static str,
        a: Point3,
        b: Point3,
        c: Point3,
        d: Point3,
        color: u32,
    ) {
        self.polygons
            .get_mut(part)
            .expect("part")
            .get_mut(face)
            .expect("face")
            .push(Polygon::new(vec![a, b, c, d], color));
    }

    fn head(&mut self) {
        self.volume_points.clear();
        let hd = self.hd;
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut j = 0.0;
            while j < 9.0 * hd {
                if !self.has(i, j, -2.0 * hd) {
                    self.put(i, j, -2.0 * hd, i, j, -2.0 * hd);
                }
                if !self.has(i, j, 6.0 * hd) {
                    self.put(i, j, 6.0 * hd, i, j, 6.0 * hd);
                }
                j += 1.0;
            }
            i += 1.0;
        }
        let mut j = 0.0;
        while j < 9.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 7.0 * hd {
                if !self.has(0.0, j, k as f64) {
                    self.put(0.0, j, k as f64, 0.0, j, k as f64);
                }
                if !self.has(8.0 * hd, j, k as f64) {
                    self.put(8.0 * hd, j, k as f64, 8.0 * hd, j, k as f64);
                }
                k += 1;
            }
            j += 1.0;
        }
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 7.0 * hd {
                if !self.has(i, 0.0, k as f64) {
                    self.put(i, 0.0, k as f64, i, 0.0, k as f64);
                }
                if !self.has(i, 8.0 * hd, k as f64) {
                    self.put(i, 8.0 * hd, k as f64, i, 8.0 * hd, k as f64);
                }
                k += 1;
            }
            i += 1.0;
        }

        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut j = 0.0;
            while j < 8.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                self.quad(
                    "head",
                    "back",
                    self.pt(i, j, -2.0 * hd),
                    self.pt(i1, j, -2.0 * hd),
                    self.pt(i1, j1, -2.0 * hd),
                    self.pt(i, j1, -2.0 * hd),
                    self.color(
                        (32.0 * hd - 1.0) as i32 - i as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                self.quad(
                    "head",
                    "front",
                    self.pt(i, j, 6.0 * hd),
                    self.pt(i1, j, 6.0 * hd),
                    self.pt(i1, j1, 6.0 * hd),
                    self.pt(i, j1, 6.0 * hd),
                    self.color((8.0 * hd) as i32 + i as i32, (8.0 * hd) as i32 + j as i32),
                );
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 8.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 6.0 * hd {
                let kf = k as f64;
                self.quad(
                    "head",
                    "right",
                    self.pt(0.0, j, kf),
                    self.pt(0.0, j, kf + 1.0),
                    self.pt(0.0, j + 1.0, kf + 1.0),
                    self.pt(0.0, j + 1.0, kf),
                    self.color(k + (2.0 * hd) as i32, (8.0 * hd) as i32 + j as i32),
                );
                self.quad(
                    "head",
                    "left",
                    self.pt(8.0 * hd, j, kf),
                    self.pt(8.0 * hd, j, kf + 1.0),
                    self.pt(8.0 * hd, j + 1.0, kf + 1.0),
                    self.pt(8.0 * hd, j + 1.0, kf),
                    self.color(
                        (24.0 * hd) as i32 - 1 - k - (2.0 * hd) as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                k += 1;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 6.0 * hd {
                let kf = k as f64;
                self.quad(
                    "head",
                    "top",
                    self.pt(i, 0.0, kf),
                    self.pt(i + 1.0, 0.0, kf),
                    self.pt(i + 1.0, 0.0, kf + 1.0),
                    self.pt(i, 0.0, kf + 1.0),
                    self.color((8.0 * hd) as i32 + i as i32, k + (2.0 * hd) as i32),
                );
                self.quad(
                    "head",
                    "bottom",
                    self.pt(i, 8.0 * hd, kf),
                    self.pt(i + 1.0, 8.0 * hd, kf),
                    self.pt(i + 1.0, 8.0 * hd, kf + 1.0),
                    self.pt(i, 8.0 * hd, kf + 1.0),
                    self.color((16.0 * hd) as i32 + i as i32, (2.0 * hd) as i32 + k),
                );
                k += 1;
            }
            i += 1.0;
        }
    }

    fn helmet(&mut self) {
        self.volume_points.clear();
        let hd = self.hd;
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut j = 0.0;
            while j < 9.0 * hd {
                if !self.has(i, j, -2.0 * hd) {
                    self.put(
                        i,
                        j,
                        -2.0 * hd,
                        i * 9.0 / 8.0 - 0.5 * hd,
                        j * 9.0 / 8.0 - 0.5 * hd,
                        -2.5 * hd,
                    );
                }
                if !self.has(i, j, 6.0 * hd) {
                    self.put(
                        i,
                        j,
                        6.0 * hd,
                        i * 9.0 / 8.0 - 0.5 * hd,
                        j * 9.0 / 8.0 - 0.5 * hd,
                        6.5 * hd,
                    );
                }
                j += 1.0;
            }
            i += 1.0;
        }
        let mut j = 0.0;
        while j < 9.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 7.0 * hd {
                let kf = k as f64;
                if !self.has(0.0, j, kf) {
                    self.put(
                        0.0,
                        j,
                        kf,
                        -0.5 * hd,
                        j * 9.0 / 8.0 - 0.5 * hd,
                        kf * 9.0 / 8.0 - 0.5 * hd,
                    );
                }
                if !self.has(8.0 * hd, j, kf) {
                    self.put(
                        8.0 * hd,
                        j,
                        kf,
                        8.5 * hd,
                        j * 9.0 / 8.0 - 0.5 * hd,
                        kf * 9.0 / 8.0 - 0.5 * hd,
                    );
                }
                k += 1;
            }
            j += 1.0;
        }
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 7.0 * hd {
                let kf = k as f64;
                if !self.has(i, 0.0, kf) {
                    self.put(
                        i,
                        0.0,
                        kf,
                        i * 9.0 / 8.0 - 0.5 * hd,
                        -0.5 * hd,
                        kf * 9.0 / 8.0 - 0.5 * hd,
                    );
                }
                if !self.has(i, 8.0 * hd, kf) {
                    self.put(
                        i,
                        8.0 * hd,
                        kf,
                        i * 9.0 / 8.0 - 0.5 * hd,
                        8.5 * hd,
                        kf * 9.0 / 8.0 - 0.5 * hd,
                    );
                }
                k += 1;
            }
            i += 1.0;
        }

        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut j = 0.0;
            while j < 8.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                self.quad(
                    "helmet",
                    "back",
                    self.pt(i, j, -2.0 * hd),
                    self.pt(i1, j, -2.0 * hd),
                    self.pt(i1, j1, -2.0 * hd),
                    self.pt(i, j1, -2.0 * hd),
                    self.color(
                        (32.0 * hd + 32.0 * hd - 1.0) as i32 - i as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                self.quad(
                    "helmet",
                    "front",
                    self.pt(i, j, 6.0 * hd),
                    self.pt(i1, j, 6.0 * hd),
                    self.pt(i1, j1, 6.0 * hd),
                    self.pt(i, j1, 6.0 * hd),
                    self.color(
                        (32.0 * hd + 8.0 * hd) as i32 + i as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                j += 1.0;
            }
            i += 1.0;
        }
        let mut j = 0.0;
        while j < 8.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 6.0 * hd {
                let kf = k as f64;
                self.quad(
                    "helmet",
                    "right",
                    self.pt(0.0, j, kf),
                    self.pt(0.0, j, kf + 1.0),
                    self.pt(0.0, j + 1.0, kf + 1.0),
                    self.pt(0.0, j + 1.0, kf),
                    self.color(
                        (32.0 * hd) as i32 + k + (2.0 * hd) as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                self.quad(
                    "helmet",
                    "left",
                    self.pt(8.0 * hd, j, kf),
                    self.pt(8.0 * hd, j, kf + 1.0),
                    self.pt(8.0 * hd, j + 1.0, kf + 1.0),
                    self.pt(8.0 * hd, j + 1.0, kf),
                    self.color(
                        (32.0 * hd + 24.0 * hd - 1.0) as i32 - k - (2.0 * hd) as i32,
                        (8.0 * hd) as i32 + j as i32,
                    ),
                );
                k += 1;
            }
            j += 1.0;
        }
        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut k = -(2.0 * hd) as i32;
            while (k as f64) < 6.0 * hd {
                let kf = k as f64;
                self.quad(
                    "helmet",
                    "top",
                    self.pt(i, 0.0, kf),
                    self.pt(i + 1.0, 0.0, kf),
                    self.pt(i + 1.0, 0.0, kf + 1.0),
                    self.pt(i, 0.0, kf + 1.0),
                    self.color(
                        (32.0 * hd + 8.0 * hd) as i32 + i as i32,
                        k + (2.0 * hd) as i32,
                    ),
                );
                self.quad(
                    "helmet",
                    "bottom",
                    self.pt(i, 8.0 * hd, kf),
                    self.pt(i + 1.0, 8.0 * hd, kf),
                    self.pt(i + 1.0, 8.0 * hd, kf + 1.0),
                    self.pt(i, 8.0 * hd, kf + 1.0),
                    self.color(
                        (32.0 * hd + 16.0 * hd) as i32 + i as i32,
                        (2.0 * hd) as i32 + k,
                    ),
                );
                k += 1;
            }
            i += 1.0;
        }
    }

    /// 生成一个 8×12×4 的盒子顶点（torso / 四肢共用）。
    ///
    /// `key2`/`x2` 复刻上游「守卫检查一个键、写入另一个键」的细节：
    /// torso 用 `8hd`，四肢用 `4hd`。
    fn build_volume(&mut self, x0: f64, y0: f64, key2: f64, x2: f64, z_end: f64) {
        self.volume_points.clear();
        let hd = self.hd;
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut j = 0.0;
            while j < 13.0 * hd {
                for k in [0.0, z_end] {
                    if !self.has(i, j, k) {
                        self.put(i, j, k, x0 + i, y0 + j, k);
                    }
                }
                j += 1.0;
            }
            i += 1.0;
        }
        let mut j = 0.0;
        while j < 13.0 * hd {
            let mut k = 0.0;
            while k < 5.0 * hd {
                if !self.has(0.0, j, k) {
                    self.put(0.0, j, k, x0, y0 + j, k);
                }
                if !self.has(key2, j, k) {
                    self.put(key2, j, k, x2, y0 + j, k);
                }
                k += 1.0;
            }
            j += 1.0;
        }
        let mut i = 0.0;
        while i < 9.0 * hd {
            let mut k = 0.0;
            while k < 5.0 * hd {
                if !self.has(i, 0.0, k) {
                    self.put(i, 0.0, k, x0 + i, y0, k);
                }
                if !self.has(i, 12.0 * hd, k) {
                    self.put(i, 12.0 * hd, k, x0 + i, y0 + 12.0 * hd, k);
                }
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn torso(&mut self) {
        let hd = self.hd;
        self.build_volume(0.0, 8.0 * hd, 8.0 * hd, 8.0 * hd, 4.0 * hd);

        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut j = 0.0;
            while j < 12.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                let a = self.pt(i, j, 0.0);
                let b = self.pt(i1, j, 0.0);
                let c = self.pt(i1, j1, 0.0);
                let d = self.pt(i, j1, 0.0);
                let color = self.color(
                    (40.0 * hd - 1.0) as i32 - i as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("torso", "back", a, b, c, d, color);

                let a = self.pt(i, j, 4.0 * hd);
                let b = self.pt(i1, j, 4.0 * hd);
                let c = self.pt(i1, j1, 4.0 * hd);
                let d = self.pt(i, j1, 4.0 * hd);
                let color =
                    self.color((20.0 * hd) as i32 + i as i32, (20.0 * hd) as i32 + j as i32);
                self.quad("torso", "front", a, b, c, d, color);
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 12.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let k1 = k + 1.0;
                let a = self.pt(0.0, j, k);
                let b = self.pt(0.0, j, k1);
                let c = self.pt(0.0, j + 1.0, k1);
                let d = self.pt(0.0, j + 1.0, k);
                let color =
                    self.color((16.0 * hd) as i32 + k as i32, (20.0 * hd) as i32 + j as i32);
                self.quad("torso", "right", a, b, c, d, color);

                let a = self.pt(8.0 * hd, j, k);
                let b = self.pt(8.0 * hd, j, k1);
                let c = self.pt(8.0 * hd, j + 1.0, k1);
                let d = self.pt(8.0 * hd, j + 1.0, k);
                let color = self.color(
                    (32.0 * hd) as i32 - 1 - k as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("torso", "left", a, b, c, d, color);
                k += 1.0;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < 8.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let (i1, k1) = (i + 1.0, k + 1.0);
                let a = self.pt(i, 0.0, k);
                let b = self.pt(i1, 0.0, k);
                let c = self.pt(i1, 0.0, k1);
                let d = self.pt(i, 0.0, k1);
                let color =
                    self.color((20.0 * hd) as i32 + i as i32, (16.0 * hd) as i32 + k as i32);
                self.quad("torso", "top", a, b, c, d, color);

                let a = self.pt(i, 12.0 * hd, k);
                let b = self.pt(i1, 12.0 * hd, k);
                let c = self.pt(i1, 12.0 * hd, k1);
                let d = self.pt(i, 12.0 * hd, k1);
                let color = self.color(
                    (28.0 * hd) as i32 + i as i32,
                    (20.0 * hd) as i32 - 1 - k as i32,
                );
                self.quad("torso", "bottom", a, b, c, d, color);
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn right_arm(&mut self) {
        let hd = self.hd;
        self.build_volume(-4.0 * hd, 8.0 * hd, 4.0 * hd, 0.0, 4.0 * hd);

        let width = if self.alex { 3.0 * hd } else { 4.0 * hd };
        let mut i = 0.0;
        while i < width {
            let mut j = 0.0;
            while j < 12.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                let back_u = if self.alex {
                    (51.0 * hd - 1.0) as i32 - i as i32
                } else {
                    (56.0 * hd - 1.0) as i32 - i as i32
                };
                let a = self.pt(i, j, 0.0);
                let b = self.pt(i1, j, 0.0);
                let c = self.pt(i1, j1, 0.0);
                let d = self.pt(i, j1, 0.0);
                let color = self.color(back_u, (20.0 * hd) as i32 + j as i32);
                self.quad("rightArm", "back", a, b, c, d, color);

                let a = self.pt(i, j, 4.0 * hd);
                let b = self.pt(i1, j, 4.0 * hd);
                let c = self.pt(i1, j1, 4.0 * hd);
                let d = self.pt(i, j1, 4.0 * hd);
                let color =
                    self.color((44.0 * hd) as i32 + i as i32, (20.0 * hd) as i32 + j as i32);
                self.quad("rightArm", "front", a, b, c, d, color);
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 12.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let k1 = k + 1.0;
                let right_u = if self.alex { 47.0 } else { 40.0 };
                let a = self.pt(0.0, j, k);
                let b = self.pt(0.0, j, k1);
                let c = self.pt(0.0, j + 1.0, k1);
                let d = self.pt(0.0, j + 1.0, k);
                let color = self.color(
                    (right_u * hd) as i32 + k as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("rightArm", "right", a, b, c, d, color);

                let left_u = if self.alex { 40.0 } else { 52.0 };
                let a = self.pt(4.0 * hd, j, k);
                let b = self.pt(4.0 * hd, j, k1);
                let c = self.pt(4.0 * hd, j + 1.0, k1);
                let d = self.pt(4.0 * hd, j + 1.0, k);
                let color = self.color(
                    (left_u * hd - 1.0) as i32 - k as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("rightArm", "left", a, b, c, d, color);
                k += 1.0;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < width {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let (i1, k1) = (i + 1.0, k + 1.0);
                let a = self.pt(i, 0.0, k);
                let b = self.pt(i1, 0.0, k);
                let c = self.pt(i1, 0.0, k1);
                let d = self.pt(i, 0.0, k1);
                let color =
                    self.color((44.0 * hd) as i32 + i as i32, (16.0 * hd) as i32 + k as i32);
                self.quad("rightArm", "top", a, b, c, d, color);

                let bottom_u = if self.alex { 47.0 } else { 48.0 };
                let a = self.pt(i, 12.0 * hd, k);
                let b = self.pt(i1, 12.0 * hd, k);
                let c = self.pt(i1, 12.0 * hd, k1);
                let d = self.pt(i, 12.0 * hd, k1);
                let color = self.color(
                    (bottom_u * hd) as i32 + i as i32,
                    (16.0 * hd) as i32 + k as i32,
                );
                self.quad("rightArm", "bottom", a, b, c, d, color);
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn left_arm(&mut self) {
        let hd = self.hd;
        self.build_volume(8.0 * hd, 8.0 * hd, 4.0 * hd, 12.0 * hd, 4.0 * hd);

        let width = if self.alex { 3.0 * hd } else { 4.0 * hd };
        let mut i = 0.0;
        while i < width {
            let mut j = 0.0;
            while j < 12.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                let (back_u, front_u) = if self.alex {
                    ((43.0 * hd) as i32 + i as i32, (36.0 * hd) as i32 + i as i32)
                } else if self.new_skin {
                    ((47.0 * hd) as i32 - i as i32, (36.0 * hd) as i32 + i as i32)
                } else {
                    (
                        (56.0 * hd - 1.0) as i32 - ((4.0 * hd - 1.0) as i32 - i as i32),
                        (44.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                    )
                };
                let a = self.pt(i, j, 0.0);
                let b = self.pt(i1, j, 0.0);
                let c = self.pt(i1, j1, 0.0);
                let d = self.pt(i, j1, 0.0);
                let color = self.color(back_u, (52.0 * hd) as i32 + j as i32);
                self.quad("leftArm", "back", a, b, c, d, color);

                let a = self.pt(i, j, 4.0 * hd);
                let b = self.pt(i1, j, 4.0 * hd);
                let c = self.pt(i1, j1, 4.0 * hd);
                let d = self.pt(i, j1, 4.0 * hd);
                let color = self.color(front_u, (52.0 * hd) as i32 + j as i32);
                self.quad("leftArm", "front", a, b, c, d, color);
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 12.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let k1 = k + 1.0;
                let (right_u, left_u) = if self.new_skin {
                    ((32.0 * hd) as i32 + k as i32, (43.0 * hd) as i32 - k as i32)
                } else {
                    (
                        (40.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - k as i32),
                        (52.0 * hd - 1.0) as i32 - ((4.0 * hd - 1.0) as i32 - k as i32),
                    )
                };
                let v = (20.0 * hd) as i32 + j as i32;
                if self.alex {
                    let color =
                        self.color((32.0 * hd) as i32 + k as i32, (52.0 * hd) as i32 + j as i32);
                    let a = self.pt(0.0, j, k);
                    let b = self.pt(0.0, j, k1);
                    let c = self.pt(0.0, j + 1.0, k1);
                    let d = self.pt(0.0, j + 1.0, k);
                    self.quad("leftArm", "right", a, b, c, d, color);
                    let color =
                        self.color((43.0 * hd) as i32 - k as i32, (52.0 * hd) as i32 + j as i32);
                    let a = self.pt(4.0 * hd, j, k);
                    let b = self.pt(4.0 * hd, j, k1);
                    let c = self.pt(4.0 * hd, j + 1.0, k1);
                    let d = self.pt(4.0 * hd, j + 1.0, k);
                    self.quad("leftArm", "left", a, b, c, d, color);
                } else {
                    let a = self.pt(0.0, j, k);
                    let b = self.pt(0.0, j, k1);
                    let c = self.pt(0.0, j + 1.0, k1);
                    let d = self.pt(0.0, j + 1.0, k);
                    let color = self.color(right_u, v);
                    self.quad("leftArm", "right", a, b, c, d, color);

                    let a = self.pt(4.0 * hd, j, k);
                    let b = self.pt(4.0 * hd, j, k1);
                    let c = self.pt(4.0 * hd, j + 1.0, k1);
                    let d = self.pt(4.0 * hd, j + 1.0, k);
                    let color = self.color(left_u, v);
                    self.quad("leftArm", "left", a, b, c, d, color);
                }
                k += 1.0;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < width {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let (i1, k1) = (i + 1.0, k + 1.0);
                let (top_u, bottom_u) = if self.alex {
                    ((36.0 * hd) as i32 + i as i32, (39.0 * hd) as i32 + i as i32)
                } else if self.new_skin {
                    ((36.0 * hd) as i32 + i as i32, (40.0 * hd) as i32 + i as i32)
                } else {
                    (
                        (44.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                        (48.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                    )
                };
                let a = self.pt(i, 0.0, k);
                let b = self.pt(i1, 0.0, k);
                let c = self.pt(i1, 0.0, k1);
                let d = self.pt(i, 0.0, k1);
                let color = self.color(top_u, (48.0 * hd) as i32 + k as i32);
                self.quad("leftArm", "top", a, b, c, d, color);

                let bottom_v = if self.alex || self.new_skin {
                    (48.0 * hd) as i32 + k as i32
                } else {
                    (20.0 * hd) as i32 - 1 - k as i32
                };
                let a = self.pt(i, 12.0 * hd, k);
                let b = self.pt(i1, 12.0 * hd, k);
                let c = self.pt(i1, 12.0 * hd, k1);
                let d = self.pt(i, 12.0 * hd, k1);
                let color = self.color(bottom_u, bottom_v);
                self.quad("leftArm", "bottom", a, b, c, d, color);
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn right_leg(&mut self) {
        let hd = self.hd;
        self.build_volume(0.0, 20.0 * hd, 4.0 * hd, 4.0 * hd, 4.0 * hd);

        let mut i = 0.0;
        while i < 4.0 * hd {
            let mut j = 0.0;
            while j < 12.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                let a = self.pt(i, j, 0.0);
                let b = self.pt(i1, j, 0.0);
                let c = self.pt(i1, j1, 0.0);
                let d = self.pt(i, j1, 0.0);
                let color = self.color(
                    (16.0 * hd - 1.0) as i32 - i as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("rightLeg", "back", a, b, c, d, color);

                let a = self.pt(i, j, 4.0 * hd);
                let b = self.pt(i1, j, 4.0 * hd);
                let c = self.pt(i1, j1, 4.0 * hd);
                let d = self.pt(i, j1, 4.0 * hd);
                let color = self.color((4.0 * hd) as i32 + i as i32, (20.0 * hd) as i32 + j as i32);
                self.quad("rightLeg", "front", a, b, c, d, color);
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 12.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let k1 = k + 1.0;
                let a = self.pt(0.0, j, k);
                let b = self.pt(0.0, j, k1);
                let c = self.pt(0.0, j + 1.0, k1);
                let d = self.pt(0.0, j + 1.0, k);
                let color = self.color(k as i32, (20.0 * hd) as i32 + j as i32);
                self.quad("rightLeg", "right", a, b, c, d, color);

                let a = self.pt(4.0 * hd, j, k);
                let b = self.pt(4.0 * hd, j, k1);
                let c = self.pt(4.0 * hd, j + 1.0, k1);
                let d = self.pt(4.0 * hd, j + 1.0, k);
                let color = self.color(
                    (12.0 * hd - 1.0) as i32 - k as i32,
                    (20.0 * hd) as i32 + j as i32,
                );
                self.quad("rightLeg", "left", a, b, c, d, color);
                k += 1.0;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < 4.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let (i1, k1) = (i + 1.0, k + 1.0);
                let a = self.pt(i, 0.0, k);
                let b = self.pt(i1, 0.0, k);
                let c = self.pt(i1, 0.0, k1);
                let d = self.pt(i, 0.0, k1);
                let color = self.color((4.0 * hd) as i32 + i as i32, (16.0 * hd) as i32 + k as i32);
                self.quad("rightLeg", "top", a, b, c, d, color);

                let a = self.pt(i, 12.0 * hd, k);
                let b = self.pt(i1, 12.0 * hd, k);
                let c = self.pt(i1, 12.0 * hd, k1);
                let d = self.pt(i, 12.0 * hd, k1);
                let color = self.color((8.0 * hd) as i32 + i as i32, (16.0 * hd) as i32 + k as i32);
                self.quad("rightLeg", "bottom", a, b, c, d, color);
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn left_leg(&mut self) {
        let hd = self.hd;
        self.build_volume(4.0 * hd, 20.0 * hd, 4.0 * hd, 8.0 * hd, 4.0 * hd);

        let mut i = 0.0;
        while i < 4.0 * hd {
            let mut j = 0.0;
            while j < 12.0 * hd {
                let (i1, j1) = (i + 1.0, j + 1.0);
                let (back_u, front_u) = if self.new_skin {
                    ((31.0 * hd) as i32 - i as i32, (20.0 * hd) as i32 + i as i32)
                } else {
                    (
                        (16.0 * hd - 1.0) as i32 - ((4.0 * hd - 1.0) as i32 - i as i32),
                        (4.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                    )
                };
                let v = if self.new_skin {
                    (52.0 * hd) as i32 + j as i32
                } else {
                    (20.0 * hd) as i32 + j as i32
                };
                let a = self.pt(i, j, 0.0);
                let b = self.pt(i1, j, 0.0);
                let c = self.pt(i1, j1, 0.0);
                let d = self.pt(i, j1, 0.0);
                let color = self.color(back_u, v);
                self.quad("leftLeg", "back", a, b, c, d, color);

                let a = self.pt(i, j, 4.0 * hd);
                let b = self.pt(i1, j, 4.0 * hd);
                let c = self.pt(i1, j1, 4.0 * hd);
                let d = self.pt(i, j1, 4.0 * hd);
                let color = self.color(front_u, v);
                self.quad("leftLeg", "front", a, b, c, d, color);
                j += 1.0;
            }
            i += 1.0;
        }

        let mut j = 0.0;
        while j < 12.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let k1 = k + 1.0;
                let (right_u, left_u) = if self.new_skin {
                    ((16.0 * hd) as i32 + k as i32, (27.0 * hd) as i32 - k as i32)
                } else {
                    (
                        (4.0 * hd - 1.0) as i32 - k as i32,
                        (12.0 * hd - 1.0) as i32 - ((4.0 * hd - 1.0) as i32 - k as i32),
                    )
                };
                let v = if self.new_skin {
                    (52.0 * hd) as i32 + j as i32
                } else {
                    (20.0 * hd) as i32 + j as i32
                };
                let a = self.pt(0.0, j, k);
                let b = self.pt(0.0, j, k1);
                let c = self.pt(0.0, j + 1.0, k1);
                let d = self.pt(0.0, j + 1.0, k);
                let color = self.color(right_u, v);
                self.quad("leftLeg", "right", a, b, c, d, color);

                let a = self.pt(4.0 * hd, j, k);
                let b = self.pt(4.0 * hd, j, k1);
                let c = self.pt(4.0 * hd, j + 1.0, k1);
                let d = self.pt(4.0 * hd, j + 1.0, k);
                let color = self.color(left_u, v);
                self.quad("leftLeg", "left", a, b, c, d, color);
                k += 1.0;
            }
            j += 1.0;
        }

        let mut i = 0.0;
        while i < 4.0 * hd {
            let mut k = 0.0;
            while k < 4.0 * hd {
                let (i1, k1) = (i + 1.0, k + 1.0);
                let (top_u, bottom_u) = if self.new_skin {
                    ((20.0 * hd) as i32 + i as i32, (24.0 * hd) as i32 + i as i32)
                } else {
                    (
                        (4.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                        (8.0 * hd) as i32 + ((4.0 * hd - 1.0) as i32 - i as i32),
                    )
                };
                let top_v = if self.new_skin {
                    (48.0 * hd) as i32 + k as i32
                } else {
                    (16.0 * hd) as i32 + k as i32
                };
                let bottom_v = if self.new_skin {
                    (48.0 * hd) as i32 + k as i32
                } else {
                    (20.0 * hd) as i32 - 1 - k as i32
                };
                let a = self.pt(i, 0.0, k);
                let b = self.pt(i1, 0.0, k);
                let c = self.pt(i1, 0.0, k1);
                let d = self.pt(i, 0.0, k1);
                let color = self.color(top_u, top_v);
                self.quad("leftLeg", "top", a, b, c, d, color);

                let a = self.pt(i, 12.0 * hd, k);
                let b = self.pt(i1, 12.0 * hd, k);
                let c = self.pt(i1, 12.0 * hd, k1);
                let d = self.pt(i, 12.0 * hd, k1);
                let color = self.color(bottom_u, bottom_v);
                self.quad("leftLeg", "bottom", a, b, c, d, color);
                k += 1.0;
            }
            i += 1.0;
        }
    }

    fn member_rotation(&mut self) {
        self.rotate_part("head", 4.0, 8.0, 2.0);
        self.rotate_part("helmet", 4.0, 8.0, 2.0);
        if !self.head_only {
            self.rotate_part("rightArm", -2.0, 8.0, 2.0);
            self.rotate_part("leftArm", 10.0, 8.0, 2.0);
            let right_leg = self.member_angles["rightLeg"];
            let z = if right_leg.sin_alpha < 0.0 { 0.0 } else { 4.0 };
            self.rotate_part("rightLeg", 2.0, 20.0, z);
            let left_leg = self.member_angles["leftLeg"];
            let z = if left_leg.sin_alpha < 0.0 { 0.0 } else { 4.0 };
            self.rotate_part("leftLeg", 6.0, 20.0, z);
        }
    }

    fn rotate_part(&mut self, part: &'static str, dx: f64, dy: f64, dz: f64) {
        let angles = self.member_angles[part];
        if let Some(faces) = self.polygons.get_mut(part) {
            for polygons in faces.values_mut() {
                for polygon in polygons {
                    polygon.pre_project(
                        dx,
                        dy,
                        dz,
                        angles.cos_alpha,
                        angles.sin_alpha,
                        angles.cos_omega,
                        angles.sin_omega,
                    );
                }
            }
        }
    }

    fn create_projection_plan(&mut self) {
        let (ca, sa, co, so) = (
            self.cos_alpha,
            self.sin_alpha,
            self.cos_omega,
            self.sin_omega,
        );
        let mut polygons = std::mem::take(&mut self.polygons);
        for faces in polygons.values_mut() {
            for items in faces.values_mut() {
                for polygon in items {
                    if !polygon.projected {
                        polygon.project(ca, sa, co, so, &mut self.bounds);
                    }
                }
            }
        }
        self.polygons = polygons;
    }

    fn display_image(&self) -> Canvas {
        let min_x = self.bounds.min_x;
        let min_y = self.bounds.min_y;
        let width = self.bounds.max_x - self.bounds.min_x;
        let height = self.bounds.max_y - self.bounds.min_y;
        let ratio2 = self.ratio * 2.0;

        let src_width_d = ratio2 * width + 1.0;
        let src_height_d = ratio2 * height + 1.0;
        let mut image = Canvas::new(src_width_d as i32, src_height_d as i32);

        for (piece, faces) in self.get_display_order() {
            for face in faces {
                if let Some(polygons) = self.polygons.get(piece).and_then(|m| m.get(face)) {
                    for polygon in polygons {
                        polygon.draw(&mut image, min_x, min_y, ratio2);
                    }
                }
            }
        }

        let real_width = (src_width_d / 2.0) as i32;
        let real_height = (src_height_d / 2.0) as i32;
        let mut dest = Canvas::new(real_width, real_height);
        dest.resample_region(
            &image,
            0,
            0,
            real_width,
            real_height,
            0,
            0,
            image.width,
            image.height,
        );
        dest
    }

    fn get_display_order(&self) -> Vec<(&'static str, Vec<&'static str>)> {
        let is_front = |face: &str| self.front_faces.contains(&face);
        let mut order: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
        let back = || self.back_faces.clone();
        let front = |part: &'static str| self.visible_faces[part].front.clone();

        if is_front("top") {
            if is_front("right") {
                order.extend([
                    ("leftLeg", back()),
                    ("leftLeg", front("leftLeg")),
                    ("rightLeg", back()),
                    ("rightLeg", front("rightLeg")),
                    ("leftArm", back()),
                    ("leftArm", front("leftArm")),
                    ("torso", back()),
                    ("torso", front("torso")),
                    ("rightArm", back()),
                    ("rightArm", front("rightArm")),
                ]);
            } else {
                order.extend([
                    ("rightLeg", back()),
                    ("rightLeg", front("rightLeg")),
                    ("leftLeg", back()),
                    ("leftLeg", front("leftLeg")),
                    ("rightArm", back()),
                    ("rightArm", front("rightArm")),
                    ("torso", back()),
                    ("torso", front("torso")),
                    ("leftArm", back()),
                    ("leftArm", front("leftArm")),
                ]);
            }
            order.extend([
                ("helmet", back()),
                ("head", back()),
                ("head", front("head")),
                ("helmet", front("head")),
            ]);
        } else {
            order.extend([
                ("helmet", back()),
                ("head", back()),
                ("head", front("head")),
                ("helmet", front("head")),
            ]);
            if is_front("right") {
                order.extend([
                    ("leftArm", back()),
                    ("leftArm", front("leftArm")),
                    ("torso", back()),
                    ("torso", front("torso")),
                    ("rightArm", back()),
                    ("rightArm", front("rightArm")),
                    ("leftLeg", back()),
                    ("leftLeg", front("leftLeg")),
                    ("rightLeg", back()),
                    ("rightLeg", front("rightLeg")),
                ]);
            } else {
                order.extend([
                    ("rightArm", back()),
                    ("rightArm", front("rightArm")),
                    ("torso", back()),
                    ("torso", front("torso")),
                    ("leftArm", back()),
                    ("leftArm", front("leftArm")),
                    ("rightLeg", back()),
                    ("rightLeg", front("rightLeg")),
                    ("leftLeg", back()),
                    ("leftLeg", front("leftLeg")),
                ]);
            }
        }
        order
    }
}

/// `render3dAvatar($skin, $ratio)`：hR=45，vR=-25，只画头部及以下全身。
pub fn render_3d_avatar(skin: &RgbaImage, ratio: f64) -> RgbaImage {
    SkinRenderer::new(ratio, true, 45.0)
        .render(skin, false)
        .to_rgba()
}

/// GD `imagecopyresampled` 风格的双线性缩放（与 PHP/Go 的最终 `resize()` 对齐）。
pub fn resize_bilinear(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let width = width.max(1) as i32;
    let height = height.max(1) as i32;
    let src = Canvas::from_rgba(source);
    let mut dest = Canvas::new(width, height);
    dest.resample_region(&src, 0, 0, width, height, 0, 0, src.width, src.height);
    dest.to_rgba()
}

fn java_round(value: f32) -> i32 {
    (value as f64 + 0.5).floor() as i32
}

fn copy_self(image: &mut Canvas, dx: i32, dy: i32, sx: i32, sy: i32, w: i32, h: i32) {
    let mut snapshot = Vec::with_capacity((w * h) as usize);
    for j in 0..h {
        for i in 0..w {
            snapshot.push(((dx + i), (dy + j), image.pixel_at(sx + i, sy + j)));
        }
    }
    for (x, y, color) in snapshot {
        image.blend_pixel(x, y, color);
    }
}

fn array_diff(all: &[&'static str], remove: &[&'static str]) -> Vec<&'static str> {
    all.iter()
        .copied()
        .filter(|face| !remove.contains(face))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_skin() -> RgbaImage {
        let mut skin = RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 0]));
        // 头部正面 / 身体正面 / 腿，给 3D 投影提供明显色块。
        for y in 8..16 {
            for x in 8..16 {
                skin.put_pixel(x, y, image::Rgba([200, 40, 40, 255]));
            }
        }
        for y in 20..32 {
            for x in 20..28 {
                skin.put_pixel(x, y, image::Rgba([40, 120, 200, 255]));
            }
        }
        for y in 20..32 {
            for x in 4..12 {
                skin.put_pixel(x, y, image::Rgba([40, 200, 80, 255]));
            }
        }
        skin
    }

    #[test]
    fn renders_non_empty_3d_avatar() {
        let image = render_3d_avatar(&sample_skin(), 25.0);
        assert!(image.width() > 0);
        assert!(image.height() > 0);
        assert!(image.pixels().any(|pixel| pixel[3] > 0));
    }

    #[test]
    fn matches_go_oracle_dimensions() {
        // Go 端口 `Render3dAvatar(skin, 25)` 的实测输出；像素级对比也一致。
        let image = render_3d_avatar(&sample_skin(), 25.0);
        assert_eq!(image.dimensions(), (318, 338));
    }

    #[test]
    fn isometric_avatar_differs_from_2d_head() {
        let skin = sample_skin();
        let three_d = render_3d_avatar(&skin, 25.0);
        let two_d = crate::support::skin::head(&skin);
        assert_ne!(three_d.dimensions(), two_d.dimensions());
    }
}
