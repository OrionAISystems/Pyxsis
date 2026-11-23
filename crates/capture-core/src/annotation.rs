use crate::{store, Region};
use anyhow::{bail, Context, Result};
use resvg::{tiny_skia, usvg};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Write as _,
    fs::File,
    io::{BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};
use windows_sys::Win32::Foundation::HWND;

const MAX_SPEC_BYTES: usize = 512 * 1024;
const MAX_POINTS: usize = 12_000;
static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
    let mut fonts = usvg::fontdb::Database::new();
    fonts.load_system_fonts();
    Arc::new(fonts)
});

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    version: u32,
    marks: Vec<Mark>,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
struct Point(f32, f32);

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Mark {
    Arrow {
        from: Point,
        to: Point,
        color: String,
        width: f32,
    },
    Rectangle {
        from: Point,
        to: Point,
        color: String,
        width: f32,
    },
    Freehand {
        points: Vec<Point>,
        color: String,
        width: f32,
    },
    Text {
        at: Point,
        text: String,
        color: String,
        size: f32,
    },
}

#[derive(Serialize)]
pub struct Export {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub bytes: u64,
    pub coordinate_space: &'static str,
}

pub fn read_spec(path: &Path) -> Result<Spec> {
    let mut json = Vec::new();
    File::open(path)?
        .take((MAX_SPEC_BYTES + 1) as u64)
        .read_to_end(&mut json)?;
    if json.len() > MAX_SPEC_BYTES {
        bail!("Annotation spec exceeds 512 KiB");
    }
    serde_json::from_slice(&json).context("Invalid annotation spec")
}

fn check_size(width: u32, height: u32) -> Result<()> {
    let image = Region {
        x: 0,
        y: 0,
        width,
        height,
    };
    image.validate(image)?;
    Ok(())
}

fn check_style(color: &str, size: f32, max: f32) -> Result<()> {
    if color.len() != 7
        || !color.starts_with('#')
        || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        bail!("Annotation colors must use #RRGGBB");
    }
    if !(1.0..=max).contains(&size) {
        bail!("Annotation width or text size is out of range");
    }
    Ok(())
}

fn tree(spec: &Spec, width: u32, height: u32) -> Result<usvg::Tree> {
    check_size(width, height)?;
    if spec.version != 1 {
        bail!("Unsupported annotation version {}", spec.version);
    }
    if spec.marks.len() > 256 {
        bail!("Use at most 256 annotations per image");
    }
    let check_point = |Point(x, y): Point| {
        if !(0.0..=width as f32).contains(&x) || !(0.0..=height as f32).contains(&y) {
            bail!("Annotation coordinates must be inside the image");
        }
        Ok(())
    };
    let mut svg =
        format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}">"#);
    let mut total_points = 0;
    for mark in &spec.marks {
        match mark {
            Mark::Arrow {
                from,
                to,
                color,
                width,
            } => {
                check_point(*from)?;
                check_point(*to)?;
                check_style(color, *width, 128.0)?;
                let Point(x, y) = *from;
                let Point(end_x, end_y) = *to;
                let length = (end_x - x).hypot(end_y - y);
                if length < 1.0 {
                    bail!("An arrow needs two distinct points");
                }
                let dx = (end_x - x) / length;
                let dy = (end_y - y) / length;
                let head = (width * 4.0).max(8.0).min(length);
                let base_x = end_x - dx * head;
                let base_y = end_y - dy * head;
                let wing = head * 0.48;
                let left = (base_x - dy * wing, base_y + dx * wing);
                let right = (base_x + dy * wing, base_y - dx * wing);
                write!(
                    svg,
                    r#"<path d="M{x} {y} L{} {}" fill="none" stroke="{color}" stroke-width="{width}" stroke-linecap="round"/><path d="M{end_x} {end_y} L{} {} L{} {} Z" fill="{color}"/>"#,
                    end_x - dx * head * 0.7,
                    end_y - dy * head * 0.7,
                    left.0,
                    left.1,
                    right.0,
                    right.1
                )?;
            }
            Mark::Rectangle {
                from,
                to,
                color,
                width,
            } => {
                check_point(*from)?;
                check_point(*to)?;
                check_style(color, *width, 128.0)?;
                let Point(x, y) = *from;
                let Point(end_x, end_y) = *to;
                write!(
                    svg,
                    r#"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{color}" stroke-width="{width}" stroke-linejoin="round"/>"#,
                    x.min(end_x),
                    y.min(end_y),
                    (end_x - x).abs(),
                    (end_y - y).abs()
                )?;
            }
            Mark::Freehand {
                points,
                color,
                width,
            } => {
                check_style(color, *width, 128.0)?;
                total_points += points.len();
                if points.len() < 2 || total_points > MAX_POINTS {
                    bail!("A stroke needs at least two points. Use at most {MAX_POINTS} points across all strokes.");
                }
                write!(
                    svg,
                    r#"<polyline fill="none" stroke="{color}" stroke-width="{width}" stroke-linecap="round" stroke-linejoin="round" points=""#
                )?;
                for &Point(x, y) in points {
                    check_point(Point(x, y))?;
                    write!(svg, "{x},{y} ")?;
                }
                svg.push_str("\"/>");
            }
            Mark::Text {
                at,
                text,
                color,
                size,
            } => {
                check_point(*at)?;
                check_style(color, *size, 512.0)?;
                if text.trim().is_empty()
                    || text.chars().count() > 200
                    || text.chars().any(char::is_control)
                {
                    bail!("Text must be a single line of 1 to 200 characters");
                }
                let Point(x, y) = *at;
                write!(
                    svg,
                    r#"<text x="{x}" y="{}" fill="{color}" font-family="Segoe UI, Arial, sans-serif" font-weight="700" font-size="{size}" xml:space="preserve">"#,
                    y + size * 0.83
                )?;
                for character in text.chars() {
                    match character {
                        '&' => svg.push_str("&amp;"),
                        '<' => svg.push_str("&lt;"),
                        '>' => svg.push_str("&gt;"),
                        _ => svg.push(character),
                    }
                }
                svg.push_str("</text>");
            }
        }
    }
    svg.push_str("</svg>");
    let options = usvg::Options {
        fontdb: FONTS.clone(),
        ..Default::default()
    };
    usvg::Tree::from_str(&svg, &options).context("Could not render annotations")
}

pub fn preview(spec: &Spec, width: u32, height: u32) -> Result<String> {
    Ok(tree(spec, width, height)?.to_string(&usvg::WriteOptions::default()))
}

pub struct AnnotatedImage {
    pub width: u32,
    pub height: u32,
    rgba: Vec<u8>,
}

pub fn render(input: &Path, spec: &Spec) -> Result<AnnotatedImage> {
    let mut decoder = png::Decoder::new(BufReader::new(File::open(input)?));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .context("Input must be a readable PNG")?;
    let (width, height) = reader.info().size();
    check_size(width, height)?;
    if spec.marks.is_empty() {
        bail!("Add at least one annotation before exporting");
    }
    let overlay = tree(spec, width, height)?;
    let mut pixels = vec![0; reader.output_buffer_size().context("PNG is too large")?];
    let frame = reader.next_frame(&mut pixels)?;
    pixels.truncate(frame.buffer_size());
    let mut rgba = match frame.color_type {
        png::ColorType::Rgba => pixels,
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[gray, alpha]| [gray, gray, gray, alpha])
            .collect(),
        png::ColorType::Grayscale => pixels.into_iter().flat_map(|p| [p, p, p, 255]).collect(),
        png::ColorType::Indexed => bail!("PNG palette could not be expanded"),
    };
    let mut layer =
        tiny_skia::Pixmap::new(width, height).context("Could not allocate annotation layer")?;
    resvg::render(
        &overlay,
        tiny_skia::Transform::identity(),
        &mut layer.as_mut(),
    );
    let (base_pixels, _) = rgba.as_chunks_mut::<4>();
    let (mark_pixels, _) = layer.data().as_chunks::<4>();
    for (base, mark) in base_pixels.iter_mut().zip(mark_pixels) {
        let alpha = u32::from(mark[3]);
        if alpha == 0 {
            continue;
        }
        let base_weight = (255 - alpha) * u32::from(base[3]);
        let output_weight = alpha * 255 + base_weight;
        for channel in 0..3 {
            // The overlay already uses premultiplied alpha. PNG uses straight alpha.
            let value = (u32::from(mark[channel]) * 255 * 255
                + u32::from(base[channel]) * base_weight
                + output_weight / 2)
                / output_weight;
            base[channel] = value.min(255) as u8;
        }
        base[3] = ((output_weight + 127) / 255) as u8;
    }
    Ok(AnnotatedImage {
        width,
        height,
        rgba,
    })
}

impl AnnotatedImage {
    pub fn save(&self, output: PathBuf) -> Result<Export> {
        let path = store::output_path("screenshot", Some(output))?;
        let mut temp =
            tempfile::NamedTempFile::new_in(path.parent().context("Output has no parent")?)?;
        self.write_png(temp.as_file_mut())?;
        temp.as_file().sync_all()?;
        let bytes = temp.as_file().metadata()?.len();
        temp.persist_noclobber(&path)
            .context("Could not save annotation without overwriting an existing file")?;
        store::log_event(
            "annotation.saved",
            &format!("{}x{}", self.width, self.height),
        );
        Ok(Export {
            path,
            width: self.width,
            height: self.height,
            bytes,
            coordinate_space: "image_pixels",
        })
    }

    pub fn copy_to_clipboard(&self, owner: isize) -> Result<()> {
        let stride = self.width as usize * 4;
        let mut dib = Vec::with_capacity(self.rgba.len());
        for row in self.rgba.rchunks_exact(stride) {
            for &[r, g, b, a] in row.as_chunks::<4>().0 {
                dib.extend_from_slice(&[b, g, r, a]);
            }
        }
        let region = Region {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        };
        crate::desktop::copy_bitmap(&dib, region, owner as HWND)
    }

    fn write_png(&self, output: impl Write) -> Result<()> {
        let mut encoder = png::Encoder::new(output, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&self.rgba)?;
        writer.finish()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_on_original_pixels_and_preserves_the_source_png() -> Result<()> {
        let mut source = tempfile::NamedTempFile::new()?;
        let mut encoder = png::Encoder::new(source.as_file_mut(), 48, 32);
        encoder.set_color(png::ColorType::Rgb);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&[255; 48 * 32 * 3])?;
        writer.finish()?;
        let before = std::fs::read(source.path())?;

        let spec = serde_json::from_str(
            r##"{"version":1,"marks":[{"type":"rectangle","from":[8,8],"to":[28,24],"color":"#ff0020","width":4}]}"##,
        )?;
        let image = render(source.path(), &spec)?;
        let at = |x: usize, y: usize| &image.rgba[(y * 48 + x) * 4..][..4];
        assert_eq!((image.width, image.height), (48, 32));
        assert_eq!(at(2, 2), &[255, 255, 255, 255]);
        assert_eq!(at(16, 8), &[255, 0, 32, 255]);
        assert_eq!(at(16, 16), &[255, 255, 255, 255]);
        assert_eq!(std::fs::read(source.path())?, before);
        Ok(())
    }

    #[test]
    fn rejects_unknown_versions_and_out_of_bounds_points() -> Result<()> {
        let unsupported = serde_json::from_str(r#"{"version":2,"marks":[]}"#)?;
        assert!(preview(&unsupported, 48, 32).is_err());
        let outside = serde_json::from_str(
            r##"{"version":1,"marks":[{"type":"arrow","from":[8,8],"to":[49,16],"color":"#ffffff","width":4}]}"##,
        )?;
        assert!(preview(&outside, 48, 32).is_err());
        Ok(())
    }
}
