//! The emoji pack's pictures, Apple's, from RealityRipple's emoji repository's
//! `apple` directory, in a font `build.rs` makes of them for the program to
//! carry; see `vendor/realityripple-emoji/README.litecord.md`. The font is
//! mapped from the program's file, so what it holds costs memory only as far
//! as it's read, and it can be given back: message text draws it as glyphs
//! ([`crate::emoji_text`]).
//!
//! A cell asks for the picture of an emoji at the size the grid draws it, and
//! gets a decoded image back at that size, which it keeps while it shows it:
//! the pictures are 112px square, and each is shrunk once to the pixels the
//! display draws, so drawing it copies it rather than scaling it again. The
//! decoded pictures are kept in a bounded cache, the least recently used let
//! go of first.

use std::{cell::RefCell, collections::HashMap};

/// The repository the program's emoji pictures come from, and its directory.
#[allow(dead_code, reason = "the release report names the source")]
pub const SOURCE: &str = "https://github.com/RealityRipple/emoji/tree/trunk/apple";
/// The commit of that repository the vendored pictures are from.
#[allow(dead_code, reason = "the release report names the commit")]
pub const SOURCE_COMMIT: &str = match option_env!("LITECORD_EMOJI_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

/// How many pictures the pack holds: its files, each a picture of one emoji or
/// one way of drawing it.
#[allow(dead_code, reason = "the release report names the count")]
pub fn assets() -> usize {
    option_env!("LITECORD_EMOJI_ASSETS")
        .and_then(|count| count.parse().ok())
        .unwrap_or(0)
}

/// The font, each picture in it a glyph. A static, as a const would be a copy in
/// the program for each place that names it.
pub static FONT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/emoji_pack.ttf"));

/// What finds a picture in [`FONT`]: a count, then for each picture where its name
/// starts and how long it is, and where it starts in the font and how long it is,
/// then the names. The names sort.
static INDEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/emoji_pack.index"));

/// The most bytes of pictures kept, which the cache lets go of at once: a filled
/// cell keeps its own, so letting them go costs a redraw of what comes next. A
/// display of twice the scale takes four times the bytes for each.
const MOST_BYTES: usize = 4 << 20;

/// The decoded pictures, each with the turn it was last asked for, so the
/// cache can let go of the one no one has asked for longest.
#[derive(Default)]
struct Pictures {
    kept: HashMap<(String, u32), (Option<slint::Image>, u64)>,
    used: u64,
    bytes: usize,
}

impl Pictures {
    /// Lets go of the least recently used pictures until the cache is back
    /// under [`MOST_BYTES`] again.
    fn evict(&mut self) {
        while self.bytes > MOST_BYTES {
            let oldest = self
                .kept
                .iter()
                .filter(|(_, (picture, _))| picture.is_some())
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone());
            let Some(key) = oldest else {
                break;
            };
            if let Some((picture, _)) = self.kept.remove(&key)
                && picture.is_some()
            {
                self.bytes -= (key.1 * key.1 * 4) as usize;
            }
        }
    }
}

thread_local! {
    static PICTURES: RefCell<Pictures> = RefCell::default();
}

/// The picture of `glyph` at `px` logical pixels, for a display of `scale`, or
/// none when the pack has no picture for it.
pub fn picture(glyph: &str, px: f32, scale: f32) -> Option<slint::Image> {
    let pixels = (px * scale).round().max(1.0) as u32;
    PICTURES.with_borrow_mut(|pictures| {
        pictures.used += 1;
        let used = pictures.used;
        let key = (glyph.to_owned(), pixels);
        if let Some((picture, last)) = pictures.kept.get_mut(&key) {
            *last = used;
            return picture.clone();
        }
        let picture = decode(glyph, pixels);
        if picture.is_some() {
            pictures.bytes += (pixels * pixels * 4) as usize;
        }
        pictures.kept.insert(key, (picture.clone(), used));
        pictures.evict();
        picture
    })
}

/// Lets go of the pictures kept, for when nothing shows them; a cell keeps its own.
pub fn forget() {
    PICTURES.with_borrow_mut(|pictures| {
        pictures.kept.clear();
        pictures.bytes = 0;
    });
}

/// Whether the pack has a picture for `glyph`, without decoding one.
pub fn has_picture(glyph: &str) -> bool {
    picture_bytes(glyph).is_some()
}

/// The picture at `pixels` square, its alpha premultiplied, as Skia draws it.
/// Drawn at 72px into a smaller cell, Skia scaled every picture again on every
/// frame a scroll drew, a quarter of the scroll's time; shrunk here once, a
/// frame copies it.
fn decode(glyph: &str, pixels: u32) -> Option<slint::Image> {
    let picture = premultiplied(&published(glyph)?);
    let (width, height) = picture.dimensions();
    let shrunk = if (width, height) == (pixels, pixels) {
        picture
    } else if pixels < width.min(height) {
        shrunk(&picture, pixels)
    } else {
        // Larger than the pack drew it, on a display of over twice the scale.
        image::imageops::resize(
            &picture,
            pixels,
            pixels,
            image::imageops::FilterType::Triangle,
        )
    };
    let buffer = slint::SharedPixelBuffer::clone_from_slice(shrunk.as_raw(), pixels, pixels);
    Some(slint::Image::from_rgba8_premultiplied(buffer))
}

/// `picture` with each colour multiplied by its alpha, so mixing pixels
/// doesn't darken the edge where a picture fades out.
fn premultiplied(picture: &image::RgbaImage) -> image::RgbaImage {
    let mut out = picture.clone();
    for pixel in out.pixels_mut() {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel.0[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    out
}

/// `picture` shrunk to `side` square, each pixel the mean of the part of the
/// picture it covers: a few microseconds, where `image`'s filters took
/// 70-120us a picture, which a scroll paid on every row that came into sight.
fn shrunk(picture: &image::RgbaImage, side: u32) -> image::RgbaImage {
    let (width, height) = picture.dimensions();
    let (across, down) = (spans(width, side), spans(height, side));
    let side = side as usize;
    let source = picture.as_raw();
    // Across first, into each source row's `side` sums.
    let mut rows = vec![0f32; height as usize * side * 4];
    for y in 0..height as usize {
        let row = &source[y * width as usize * 4..][..width as usize * 4];
        for (x, span) in across.iter().enumerate() {
            let out = &mut rows[(y * side + x) * 4..][..4];
            for &(at, weight) in span {
                for channel in 0..4 {
                    out[channel] += f32::from(row[at * 4 + channel]) * weight;
                }
            }
        }
    }
    let mut out = vec![0u8; side * side * 4];
    for (y, span) in down.iter().enumerate() {
        for x in 0..side {
            let mut sum = [0f32; 4];
            for &(at, weight) in span {
                let from = &rows[(at * side + x) * 4..][..4];
                for channel in 0..4 {
                    sum[channel] += from[channel] * weight;
                }
            }
            for channel in 0..4 {
                out[(y * side + x) * 4 + channel] = sum[channel].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(side as u32, side as u32, out).expect("a picture's pixels")
}

/// For each of `to` pixels shrunk from `from`, the source pixels it covers and
/// how much of each, which add up to one.
fn spans(from: u32, to: u32) -> Vec<Vec<(usize, f32)>> {
    let step = from as f32 / to as f32;
    (0..to)
        .map(|out| {
            let (start, end) = (out as f32 * step, (out + 1) as f32 * step);
            (start.floor() as usize..(end.ceil() as usize).min(from as usize))
                .map(|at| {
                    let covered = end.min(at as f32 + 1.0) - start.max(at as f32);
                    (at, covered / step)
                })
                .filter(|(_, weight)| *weight > 0.0)
                .collect()
        })
        .collect()
}

/// The names the pack may have given an emoji's picture: its code points in
/// lower-case hexadecimal, at least four digits each, the same with the
/// variation selectors dropped, as most of them are (`2764-fe0f.png` for ❤️,
/// `0031-fe0f-20e3.png` for 1️⃣), and for a single code point the pack draws
/// only with a selector after it, the name with `-fe0f` (`2600-fe0f.png` for ☀).
fn names(glyph: &str) -> Vec<String> {
    let code_points: Vec<String> = glyph
        .chars()
        .map(|character| format!("{:04x}", character as u32))
        .collect();
    let exact = code_points.join("-");
    let dropped = code_points
        .iter()
        .filter(|point| point.as_str() != "fe0f")
        .cloned()
        .collect::<Vec<_>>()
        .join("-");
    let mut names = vec![exact];
    if dropped != names[0] {
        names.push(dropped);
    }
    if code_points.len() == 1 {
        names.push(format!("{}-fe0f", names[0]));
    }
    names
}

/// The picture of `glyph` as the pack publishes it, without the margin it has in
/// the font.
pub(crate) fn published(glyph: &str) -> Option<image::RgbaImage> {
    use crate::emoji_font_tables::PACK_MARGIN as MARGIN;
    let bitmap =
        image::load_from_memory_with_format(picture_bytes(glyph)?, image::ImageFormat::Png)
            .ok()?
            .into_rgba8();
    let side = bitmap.width() - 2 * MARGIN;
    Some(image::imageops::crop_imm(&bitmap, MARGIN, MARGIN, side, side).to_image())
}

fn picture_bytes(glyph: &str) -> Option<&'static [u8]> {
    let (_, picture) = entry(find(glyph)?);
    Some(picture)
}

/// The code point of the glyph in [`FONT`] that draws `emoji`.
pub fn glyph(emoji: &str) -> Option<char> {
    char::from_u32(crate::emoji_font_tables::PACK_LAST - find(emoji)? as u32)
}

/// The place of the picture of `glyph` in the sorted list.
fn find(glyph: &str) -> Option<usize> {
    names(glyph).iter().find_map(|name| {
        let (mut low, mut high) = (0, count());
        while low < high {
            let middle = low + (high - low) / 2;
            match entry(middle).0.cmp(name.as_str()) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    })
}

fn count() -> usize {
    u32::from_le_bytes(INDEX[0..4].try_into().expect("the index's count")) as usize
}

/// The name and the picture at `place` in the sorted list. An entry is where its
/// name starts, u32, and how long it is, u16, then where its picture starts and
/// how long it is, u32 each.
fn entry(place: usize) -> (&'static str, &'static [u8]) {
    const ENTRY: usize = 14;
    let at = 4 + place * ENTRY;
    let u32_at =
        |at: usize| u32::from_le_bytes(INDEX[at..at + 4].try_into().expect("a number")) as usize;
    let name_at = u32_at(at);
    let name_length =
        u16::from_le_bytes(INDEX[at + 4..at + 6].try_into().expect("a name's length")) as usize;
    let picture_at = u32_at(at + 6);
    let name = std::str::from_utf8(&INDEX[name_at..name_at + name_length]).expect("a name");
    (name, &FONT[picture_at..picture_at + u32_at(at + 10)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_emoji_has_the_packs_picture_at_the_size_it_draws() {
        // Shrunk once to the pixels a cell draws, so drawing it copies it
        // rather than scaling 112px down again every frame.
        for (px, scale, side) in [
            (36., 1., 36),
            (36., 2., 72),
            (32., 1.25, 40),
            (36., 3., 108),
        ] {
            let drawn = picture("🍕", px, scale).expect("the pack draws a pizza");
            assert_eq!(
                (drawn.size().width, drawn.size().height),
                (side, side),
                "{px} logical at {scale}"
            );
        }
    }

    #[test]
    fn a_shrunk_picture_keeps_its_colour_and_its_edge() {
        // A square red to just short of half, clear after: shrunk, the colour stays red
        // where it fades, as its alpha is premultiplied, and the clear half
        // stays clear.
        let mut square = image::RgbaImage::new(72, 72);
        for (x, _, pixel) in square.enumerate_pixels_mut() {
            *pixel = if x < 35 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([255, 255, 255, 0])
            };
        }
        let small = shrunk(&premultiplied(&square), 32);
        assert_eq!(small.dimensions(), (32, 32));
        assert_eq!(small.get_pixel(0, 5).0, [255, 0, 0, 255]);
        assert_eq!(small.get_pixel(31, 5).0, [0, 0, 0, 0]);
        // The column the edge crosses is part of each.
        let edge = small.get_pixel(15, 5).0;
        assert!(
            edge[3] > 0 && edge[3] < 255 && edge[0] == edge[3] && edge[1] == 0,
            "{edge:?}"
        );
        // Each pixel's share of the source adds up to one.
        for span in spans(72, 32) {
            let sum: f32 = span.iter().map(|(_, weight)| weight).sum();
            assert!((sum - 1.0).abs() < 1e-4, "{span:?}");
        }
    }

    #[test]
    fn a_sequence_has_a_picture_of_its_own() {
        // The pack draws these as pictures, where the system font's glyph would
        // have to be shaped.
        assert!(has_picture("👨‍👩‍👧"));
        assert!(has_picture("🇩🇰"));
        assert!(has_picture("1️⃣"));
        assert!(has_picture("☀"));
        assert!(has_picture("☀️"));
        assert!(picture("👨‍👩‍👧", 36., 1.).is_some());
        // Apple names its pictures by their code points alone, so a skin tone
        // and a ZWJ sequence answer to theirs.
        assert!(has_picture("👍🏽"));
        assert!(has_picture("👩‍🦯"));
        assert!(has_picture("🧗‍♀️"));
    }

    /// The glyphs and advances the font shapes `text` into, as Slint's text
    /// shapes it.
    fn shaped(text: &str) -> Vec<(u32, i32)> {
        let font = harfrust::FontRef::new(FONT).expect("the font reads");
        let data = harfrust::ShaperData::new(&font);
        let shaper = data.shaper(&font).build();
        let mut buffer = harfrust::UnicodeBuffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let shaped = shaper.shape(buffer, harfrust::ShapeOptions::new());
        shaped
            .glyph_infos()
            .iter()
            .zip(shaped.glyph_positions())
            .map(|(info, position)| (info.glyph_id, position.x_advance))
            .collect()
    }

    /// The glyph of `emoji`'s picture in the font.
    fn picture_glyph(emoji: &str) -> u32 {
        // Pictures are glyphs 1 on, in the order of their names; their code
        // points count down from the font's last.
        crate::emoji_font_tables::PACK_LAST - glyph(emoji).expect("a picture") as u32 + 1
    }

    #[test]
    fn text_typed_as_several_code_points_shapes_into_one_picture() {
        let advance = shaped("🍕")[0].1;
        assert_eq!(shaped("🍕"), [(picture_glyph("🍕"), advance)]);
        // A family, a flag, a skin tone and a keycap, each its one picture.
        for emoji in ["👨‍👩‍👧", "🇩🇰", "👍🏽", "🏳️‍🌈", "🏴󠁧󠁢󠁳󠁣󠁴󠁿", "👩‍🦯", "🧗‍♀️", "1️⃣", "#️⃣", "*️⃣"]
        {
            assert_eq!(
                shaped(emoji),
                [(picture_glyph(emoji), advance)],
                "{emoji} is one picture"
            );
        }
        // A selector after a single emoji takes no room, and typed without
        // the selector a sequence joins all the same.
        let heart = shaped("❤️");
        assert_eq!(heart.iter().map(|(_, x)| x).sum::<i32>(), advance);
        assert_eq!(heart[0].0, picture_glyph("❤️"));
        assert_eq!(shaped("🏳‍🌈").len(), 1);
        // A keycap's digit, `#` or `*` is an empty glyph of this font: alone
        // it takes no room, and with its keycap it joins into the picture.
        for text in ["2", "#", "*"] {
            let shaped = shaped(text);
            assert!(shaped[0].0 > 0, "{text} has a glyph");
            assert_eq!(shaped[0].1, 0, "{text} takes no room");
        }
    }

    #[test]
    fn the_pack_reports_the_emoji_it_cannot_draw() {
        // The report the release notes name: which of Discord's emoji, skin
        // tones and sequences included, have no Apple picture. They stay as
        // typed; no other collection stands in for them.
        assert_eq!(assets(), count(), "the index has one entry for each file");
        let newest = emojis::UnicodeVersion::new(15, 0);
        let mut seen = std::collections::BTreeSet::new();
        for emoji in emojis::iter() {
            for emoji in std::iter::once(emoji).chain(emoji.skin_tones().into_iter().flatten()) {
                if emoji.unicode_version() <= newest {
                    seen.insert(emoji.as_str().to_owned());
                }
            }
        }
        let missing: Vec<&String> = seen
            .iter()
            .filter(|emoji| !has_picture(emoji.as_str()))
            .collect();
        println!(
            "emoji pack: {} pictures; {}/{} Unicode 15.0 emoji have no picture",
            assets(),
            missing.len(),
            seen.len()
        );
        for emoji in missing {
            println!("  {emoji}");
        }
    }

    #[test]
    fn the_names_are_the_code_points_and_those_without_variation_selectors() {
        assert_eq!(names("🍕"), ["1f355".to_owned(), "1f355-fe0f".to_owned()]);
        assert_eq!(names("❤️"), ["2764-fe0f".to_owned(), "2764".to_owned()]);
        // Text's code points are padded to four digits, as the pack names them.
        assert_eq!(
            names("1️⃣"),
            ["0031-fe0f-20e3".to_owned(), "0031-20e3".to_owned()]
        );
        // A picture the pack named without the selector is found all the same,
        // and one it draws only with a selector is found without one.
        assert!(has_picture("❤️"));
        assert!(has_picture("1️⃣"));
        assert!(has_picture("☀"));
        // One it has no picture for says so, and the cell keeps its glyph.
        assert!(!has_picture("not an emoji"));
    }
}
