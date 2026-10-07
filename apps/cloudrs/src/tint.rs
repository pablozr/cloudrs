//! The dominant colour of an image, for the page tint (ADR 0009).
//!
//! Computed in the app from the cached artwork file, off the UI thread; the
//! core never sees it.

use std::path::Path;

use gpui::{Hsla, Rgba};
use image::{ImageReader, RgbaImage, imageops::FilterType};

/// The image is shrunk to fit this many pixels a side before averaging.
const SAMPLE_SIDE: u32 = 32;

/// Weight a pure grey pixel keeps, so an all-grey cover still gets a (grey)
/// colour instead of none.
const GREY_WEIGHT: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl From<Rgb> for Hsla {
    fn from(Rgb(r, g, b): Rgb) -> Self {
        let channel = |v: u8| f32::from(v) / 255.0;
        Rgba {
            r: channel(r),
            g: channel(g),
            b: channel(b),
            a: 1.0,
        }
        .into()
    }
}

/// Decodes `path` and returns its dominant colour; `None` when the file cannot
/// be read as an image. Blocking: call it from a background task.
pub fn dominant_color(path: &Path) -> Option<Rgb> {
    // The cache names files by key, not by codec: sniff the format from the
    // bytes instead of trusting the extension.
    let image = ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let sample = image
        .resize(SAMPLE_SIDE, SAMPLE_SIDE, FilterType::Triangle)
        .to_rgba8();
    saturation_weighted_average(&sample)
}

/// Average colour where each pixel counts by how colourful it is: chroma,
/// tapering to zero toward black and white, times opacity. Greys and
/// near-black or near-white pixels barely move the result.
fn saturation_weighted_average(image: &RgbaImage) -> Option<Rgb> {
    let mut sum = [0.0_f32; 3];
    let mut total = 0.0_f32;
    for pixel in image.pixels() {
        let [r, g, b, a] = pixel.0.map(|v| f32::from(v) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let lightness = (max + min) / 2.0;
        let mid_tone = 1.0 - (2.0 * lightness - 1.0).abs();
        let weight = ((max - min) * mid_tone + GREY_WEIGHT) * a;
        for (sum, value) in sum.iter_mut().zip([r, g, b]) {
            *sum += value * weight;
        }
        total += weight;
    }
    if total <= 0.0 {
        return None;
    }
    let [r, g, b] = sum.map(|v| (v / total * 255.0).round() as u8);
    Some(Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    fn hue_degrees(Rgb(r, g, b): Rgb) -> f32 {
        let color: Hsla = Rgb(r, g, b).into();
        color.h * 360.0
    }

    #[test]
    fn grey_noise_does_not_hide_the_orange() {
        let mut image = RgbaImage::new(32, 32);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = if (x + y) % 3 == 0 {
                // Greys, near-black and near-white.
                let grey = [20, 128, 240][((x * 7 + y * 13) % 3) as usize];
                Rgba([grey, grey, grey, 255])
            } else {
                Rgba([235, 110, 20, 255])
            };
        }
        let color = saturation_weighted_average(&image).expect("a colour");
        let hue = hue_degrees(color);
        assert!((15.0..=35.0).contains(&hue), "hue {hue} is not orange");
        let Rgb(r, _, b) = color;
        assert!(r > 180 && b < 80, "{color:?} is not close to the orange");
    }

    #[test]
    fn a_grey_image_gets_a_grey_colour() {
        let image = RgbaImage::from_pixel(4, 4, Rgba([100, 100, 100, 255]));
        assert_eq!(
            saturation_weighted_average(&image),
            Some(Rgb(100, 100, 100))
        );
    }

    #[test]
    fn a_transparent_image_has_no_colour() {
        let image = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 0]));
        assert_eq!(saturation_weighted_average(&image), None);
    }

    #[test]
    fn an_unreadable_file_has_no_colour() {
        assert_eq!(dominant_color(Path::new("/does/not/exist.jpg")), None);
    }
}
