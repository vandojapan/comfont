//! Replace SVG emoji and complete keycaps with ordered layout slots.
use crate::{
    control_text::{ControlTextDefaults, parse_decorated_control_text},
    raster, render,
    shaping::{Fonts, HostFonts},
};
use aviutl2::{
    filter::{FilterProcVideo, ImageResource, OutputImageResourcePixelFormat},
    generic::HostAppHandle,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use unicode_segmentation::UnicodeSegmentation;
use windows::{
    Win32::Graphics::DirectWrite::*,
    core::{HSTRING, Interface},
};

const FONT: &str = "Comfont Emoji Marker v2";
const MARK: char = '\u{e000}';
const BLANK: char = '\u{e001}';
static READY: AtomicBool = AtomicBool::new(false);
static RESOURCE: AtomicU64 = AtomicU64::new(0);

// Explicitly enabled, debug-build-only diagnostics for the isolated test host.
pub(crate) fn trace(message: &str) {
    #[cfg(debug_assertions)]
    if let Ok(exe) = std::env::current_exe()
        && let Some(root) = exe.parent()
        && root.join("comfont-emoji-trace.enabled").is_file()
    {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("comfont-emoji-trace.txt"))
        {
            let _ = writeln!(file, "{message}");
        }
    }
    #[cfg(not(debug_assertions))]
    let _ = message;
}

pub(crate) fn register_font(host: &mut HostAppHandle) -> aviutl2::AnyResult<()> {
    let bytes = include_bytes!("../../../resource/emoji-marker.ttf");
    let path = aviutl2::config::app_data_path().join("compositefont/emoji-marker-v2.ttf");
    std::fs::create_dir_all(path.parent().unwrap())?;
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&path, bytes)?;
    }
    // Register a private collection; no Windows font installation is involved.
    unsafe {
        let factory: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let modern: IDWriteFactory5 = factory.cast()?;
        let builder = modern.CreateFontSetBuilder()?;
        let file = factory
            .CreateFontFileReference(&HSTRING::from(path.to_string_lossy().as_ref()), None)?;
        builder.AddFontFile(&file)?;
        let set = builder.CreateFontSet()?;
        let collection: IDWriteFontCollection = factory
            .cast::<IDWriteFactory3>()?
            .CreateFontCollectionFromFontSet(&set)?
            .cast()?;
        host.register_font_collection(&collection);
    }
    READY.store(true, Ordering::Release);
    Ok(())
}

struct Slot {
    color: u32,
    image: raster::RasterImage,
    cell_height: f32,
    advance: f32,
    marker_width: f32,
    offset_x: f32,
}
struct Plan {
    probe: String,
    clean: String,
    slots: Vec<Slot>,
}
pub(crate) struct Output {
    pub bitmap: render::Bitmap,
    // Deltas from the original object centre, including any bitmap expansion.
    pub cx: f32,
    pub cy: f32,
}
type Key = (Vec<(String, String)>, String, (u32, u32));
#[derive(Default)]
pub(crate) struct Cache {
    entries: VecDeque<(Key, Option<Arc<Output>>)>,
}

pub(crate) fn render(
    video: &mut FilterProcVideo<()>,
    params: &[(String, String)],
    text: &str,
    request: &render::RenderRequest,
    cache: &Mutex<Cache>,
) -> Result<Option<Arc<Output>>, String> {
    if !READY.load(Ordering::Acquire) {
        return Ok(None);
    }
    let original_size = video
        .get_image_resource_size(&ImageResource::Object)
        .map_err(|e| e.to_string())?;
    let key = (params.to_vec(), text.to_owned(), original_size);
    if let Ok(mut cache) = cache.lock()
        && let Some(index) = cache.entries.iter().position(|(k, _)| k == &key)
    {
        let entry = cache.entries.remove(index).unwrap();
        let output = entry.1.clone();
        cache.entries.push_back(entry);
        return Ok(output);
    }
    let plan = {
        trace(&format!("decorated: {text}"));
        let section = video.read_section();
        let mut coverage = crate::FontManagerGlyphCoverage::new(section);
        let Some(styles) = render::prepare_native(request, text, &mut coverage)? else {
            return Ok(None);
        };
        build_plan(text, request, &styles, &mut HostFonts::new(section))?
    };
    let output = match plan {
        Some(plan) => Some(Arc::new(paint(
            video,
            params,
            plan,
            request.align,
            original_size,
        )?)),
        None => None,
    };
    if let Ok(mut cache) = cache.lock() {
        cache.entries.push_back((key, output.clone()));
        while cache.entries.len() > 8
            || cache
                .entries
                .iter()
                .filter_map(|(_, v)| v.as_ref())
                .map(|v| v.bitmap.bgra.len())
                .sum::<usize>()
                > 64 * 1024 * 1024
        {
            cache.entries.pop_front();
        }
    }
    Ok(output)
}

fn build_plan(
    text: &str,
    request: &render::RenderRequest,
    styles: &crate::shaping::StyledText,
    fonts: &mut dyn Fonts,
) -> Result<Option<Plan>, String> {
    let parsed = parse_decorated_control_text(
        text,
        ControlTextDefaults {
            font_size: Some(request.size),
            font_family: Some(request.font.clone()),
        },
    )
    .map_err(|_| "Unsupported emoji source controls")?;
    let emoji = icu_properties::CodePointSetData::new::<icu_properties::props::Emoji>();
    let mut families = HashMap::new();
    let mut used_colors: HashSet<u32> = styles.ranges.iter().map(|s| s.style.color).collect();
    used_colors.extend([0, 0xffffff]);
    let mut plan = Plan {
        probe: String::new(),
        clean: String::new(),
        slots: Vec::new(),
    };
    let mut cursor = 0;
    let mut utf16 = 0;
    for span in parsed.spans {
        for (offset, cluster) in text[span.range.clone()].grapheme_indices(true) {
            let start = span.range.start + offset;
            let length = icu_normalizer::ComposingNormalizer::new_nfc()
                .normalize(cluster)
                .encode_utf16()
                .count();
            let style = styles
                .ranges
                .iter()
                .find(|r| r.utf16.contains(&utf16))
                .map(|r| &r.style);
            utf16 += length;
            let Some(style) = style else { continue };
            let first = cluster.chars().next().unwrap();
            let keycap = crate::keycap::is_keycap(cluster);
            // ASCII digits/#/* become emoji only as a complete keycap sequence.
            if !emoji.contains(first) || (first.is_ascii() && !keycap) {
                continue;
            }
            let is_svg = *families.entry(style.family.clone()).or_insert_with(|| {
                fonts
                    .get(&style.family)
                    .ok()
                    .and_then(|font| unsafe { font.CreateFontFace().ok() })
                    .and_then(|face| face.cast::<IDWriteFontFace4>().ok())
                    .is_some_and(|face| {
                        unsafe { face.GetGlyphImageFormats2() }
                            .contains(DWRITE_GLYPH_IMAGE_FORMATS_SVG)
                    })
            });
            // The host splits keycap marks even after profile resolution chose
            // a suitable lower-priority font. Shape and paint that entire
            // grapheme here, including COLR or monochrome fallback glyphs.
            if !is_svg && !keycap {
                continue;
            }
            let mut glyph_style = style.clone();
            glyph_style.tracking = 0.0;
            glyph_style.baseline_shift = 0.0;
            let mut image = match raster::glyph_with_style(fonts, cluster, &glyph_style) {
                Ok(image) => image,
                // An unavailable keycap must not disable other SVG emoji.
                Err(_) if keycap => continue,
                Err(error) => return Err(error),
            };
            if keycap && image.svg_layers == 0 {
                // Monochrome fallback marks can overhang the base digit's
                // advance. Reserve the complete bitmap cell so adjacent
                // keycaps cannot overlap merely because of that overhang.
                image.advance = image.width as f32;
                image.origin_x = 0.0;
            }
            trace(&format!(
                "cluster={cluster:?}; style={glyph_style:?}; SVG layers={}; advance={}",
                image.svg_layers, image.advance
            ));
            if image.svg_layers == 0 && !keycap {
                continue;
            }
            let advance = image.advance + style.tracking;
            let cell_height = style.size * style.vertical;
            if plan.slots.len() >= 256 || advance < 2.0 || cell_height < 4.0 {
                return Err("Emoji layout slot limit exceeded".into());
            }
            if plan.slots.iter().map(|s| s.image.bgra.len()).sum::<usize>() + image.bgra.len()
                > 64 * 1024 * 1024
            {
                return Err("Emoji slot images exceed 64 MiB".into());
            }
            let mut color = (0x070001 + plan.slots.len() as u32 * 104729) % 0xffffff;
            while !used_colors.insert(color) {
                color = (color + 1) % 0xffffff;
            }
            let (prefix, marker, blank, native_advance) = slot_prefix(advance, cell_height, color);
            let flags = format!(
                "{}{}",
                if style.bold { "B" } else { "" },
                if style.italic { "I" } else { "" }
            );
            let restore = format!(
                "<@{},0><s{},{},{}><tw{}><th{}><gw{}><#{:06x}>",
                style.family,
                crate::format_control_number(f64::from(style.size)),
                style.family,
                flags,
                crate::format_control_number(f64::from(style.horizontal)),
                crate::format_control_number(f64::from(style.vertical)),
                crate::format_control_number(f64::from(style.tracking)),
                style.color
            );
            for (target, marker) in [(&mut plan.probe, marker), (&mut plan.clean, blank)] {
                target.push_str(&text[cursor..start]);
                target.push_str(&prefix);
                target.push(marker);
                target.push_str(&restore);
            }
            cursor = start + cluster.len();
            plan.slots.push(Slot {
                color,
                cell_height,
                advance,
                marker_width: marker_width(advance, cell_height),
                // Remove native letter-spacing correction and the centring of
                // the narrow probe inside the original full-width rectangle.
                // This retains the user's tracking and the original SVG origin.
                offset_x: (native_advance - image.advance + marker_width(advance, cell_height)
                    - advance)
                    / 2.0,
                image,
            });
        }
    }
    if plan.slots.is_empty() {
        return Ok(None);
    }
    plan.probe.push_str(&text[cursor..]);
    plan.clean.push_str(&text[cursor..]);
    if plan.probe.len() > 65536 {
        return Err("Emoji control text exceeded 64 KiB".into());
    }
    Ok(Some(plan))
}

fn slot_prefix(advance: f32, cell_height: f32, color: u32) -> (String, char, char, f32) {
    // Keep transformed ink within the native glyph width so it cannot overlap
    // following text. Wider glyphs also avoid increasing the font's line height.
    let em = cell_height.max(advance / 32.0);
    let index = [1.0, 2.0, 4.0, 8.0, 16.0, 32.0]
        .iter()
        .position(|width| advance <= em * width)
        .unwrap_or(5);
    let width = (1_u32 << index) as f32;
    let marker = char::from_u32(MARK as u32 + index as u32 * 2).unwrap();
    let blank = char::from_u32(BLANK as u32 + index as u32 * 2).unwrap();
    let prefix = format!(
        "<@{FONT},0><@-BIS><s{},{FONT},><tw{}><th{}><gw{}><#{color:06x}>",
        crate::format_control_number(f64::from(em)),
        crate::format_control_number(f64::from(marker_width(advance, cell_height) / (em * width))),
        crate::format_control_number(f64::from(cell_height / em)),
        // Native <tw> scales the ink, not the glyph's layout advance.
        crate::format_control_number(f64::from(advance - em * width)),
    );
    (prefix, marker, blank, em * width)
}

fn marker_width(advance: f32, cell_height: f32) -> f32 {
    // AviUtl2 puts half of gw before the ink. Adjacent slots with different
    // advances therefore move their probe origins by different amounts. A full
    // cell rectangle can obscure its neighbour even though the logical cells
    // do not overlap. A narrow locator retains the same origin and advance.
    advance.min(cell_height).clamp(2.0, 8.0)
}

fn native_image(
    video: &mut FilterProcVideo<()>,
    params: &[(String, String)],
    text: &str,
    resource: &ImageResource,
) -> Result<render::Bitmap, String> {
    let mut params = params.to_vec();
    let value = params
        .iter_mut()
        .find(|(key, _)| key == "テキスト")
        .ok_or("No native text")?;
    value.1 = super::native_filter::encode_alias(text);
    let original = video.param;
    let result = (|| {
        video
            .exec_effect("テキスト", params.iter().map(|(k, v)| (k, v)), resource)
            .map_err(|e| e.to_string())?;
        let (w, h) = video
            .get_image_resource_size(resource)
            .map_err(|e| e.to_string())?;
        if w == 0 || h == 0 || w > 8192 || h > 8192 || u64::from(w) * u64::from(h) > 16_777_216 {
            return Err("Native emoji image exceeds bitmap limit".into());
        }
        let mut bgra = vec![0; (w * h * 4) as usize];
        video
            .get_image_resource_data(
                resource,
                &mut bgra,
                w,
                h,
                w * 4,
                OutputImageResourcePixelFormat::Rgba,
            )
            .map_err(|e| e.to_string())?;
        for pixel in bgra.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Ok(render::Bitmap {
            width: w as i32,
            height: h as i32,
            bgra,
        })
    })();
    video.param = original;
    result
}

fn paint(
    video: &mut FilterProcVideo<()>,
    params: &[(String, String)],
    plan: Plan,
    align: i32,
    original_size: (u32, u32),
) -> Result<Output, String> {
    let id = RESOURCE.fetch_add(1, Ordering::Relaxed);
    let probe_resource = ImageResource::Resource(format!("comfont-emoji-probe-{id}"));
    let clean_resource = ImageResource::Resource(format!("comfont-emoji-clean-{id}"));
    let result = (|| {
        let probe = native_image(video, params, &plan.probe, &probe_resource)?;
        let clean = native_image(video, params, &plan.clean, &clean_resource)?;
        // Painted final slots may extend ink bounds by one pixel. Align object
        // origins instead of requiring equal bitmap crop dimensions.
        let dx = (clean.width - probe.width) as f32 / 2.0;
        let dy = (clean.height - probe.height) as f32 / 2.0;
        trace(&format!(
            "native slots: probe={}x{}, clean={}x{}, origin offset={dx},{dy}",
            probe.width, probe.height, clean.width, clean.height
        ));
        let offset = align_native_images(&probe, &clean, &plan.slots, dx, dy)?;
        let cx = (align % 3 - 1) as f32 * (clean.width - original_size.0 as i32) as f32 / 2.0;
        let cy = (align / 3 - 1) as f32 * (clean.height - original_size.1 as i32) as f32 / 2.0;
        compose(clean, &probe, &plan.slots, cx, cy, offset)
    })();
    let _ = video.release_image_resource(&probe_resource);
    let _ = video.release_image_resource(&clean_resource);
    result
}

// exec_effect updates the SDK's image parameters, while aviutl2-rs keeps a
// snapshot in video.param. Establish the integer crop offset from unchanged
// native text pixels rather than trusting that snapshot's centre coordinates.
fn align_native_images(
    probe: &render::Bitmap,
    clean: &render::Bitmap,
    slots: &[Slot],
    dx: f32,
    dy: f32,
) -> Result<(i32, i32), String> {
    if !dx.is_finite() || !dy.is_finite() {
        return Err("Invalid native slot origin".into());
    }
    let colors: HashSet<_> = slots.iter().map(|s| s.color).collect();
    let mut mask = vec![false; (probe.width * probe.height) as usize];
    for (i, p) in probe.bgra.chunks_exact(4).enumerate() {
        let color = u32::from(p[0]) | (u32::from(p[1]) << 8) | (u32::from(p[2]) << 16);
        if p[3] > 0 && colors.contains(&color) {
            let (x, y) = (i as i32 % probe.width, i as i32 / probe.width);
            for yy in (y - 2).max(0)..(y + 3).min(probe.height) {
                for xx in (x - 2).max(0)..(x + 3).min(probe.width) {
                    mask[(yy * probe.width + xx) as usize] = true;
                }
            }
        }
    }
    let mut candidates = vec![(0, 0)];
    for x in [dx.floor() as i32, dx.ceil() as i32] {
        for y in [dy.floor() as i32, dy.ceil() as i32] {
            if !candidates.contains(&(x, y)) {
                candidates.push((x, y));
            }
        }
    }
    for (xoff, yoff) in candidates {
        let mut mismatch = false;
        for y in 0..clean.height {
            for x in 0..clean.width {
                let (px, py) = (x - xoff, y - yoff);
                let inside = px >= 0 && py >= 0 && px < probe.width && py < probe.height;
                if inside && mask[(py * probe.width + px) as usize] {
                    continue;
                }
                let cp = &clean.bgra[((y * clean.width + x) * 4) as usize..][..4];
                let pp = if inside {
                    &probe.bgra[((py * probe.width + px) * 4) as usize..][..4]
                } else {
                    &[0, 0, 0, 0]
                };
                if !(cp[3] == 0 && pp[3] == 0) && cp != pp {
                    mismatch = true;
                    break;
                }
            }
            if mismatch {
                break;
            }
        }
        if !mismatch {
            trace(&format!(
                "native text pixels agree at crop offset {xoff},{yoff}"
            ));
            return Ok((xoff, yoff));
        }
    }
    Err("Native text pixels differ between marker and blank passes".into())
}

fn compose(
    clean: render::Bitmap,
    probe: &render::Bitmap,
    slots: &[Slot],
    cx: f32,
    cy: f32,
    probe_offset: (i32, i32),
) -> Result<Output, String> {
    let colors: HashMap<u32, usize> = slots
        .iter()
        .enumerate()
        .map(|(i, s)| (s.color, i))
        .collect();
    let mut bounds = vec![(i32::MAX, i32::MAX, 0, 0); slots.len()];
    for (i, p) in probe.bgra.chunks_exact(4).enumerate() {
        if p[3] < 250 {
            continue;
        }
        let color = u32::from(p[0]) | (u32::from(p[1]) << 8) | (u32::from(p[2]) << 16);
        if let Some(&slot) = colors.get(&color) {
            let (x, y) = (
                i as i32 % probe.width + probe_offset.0,
                i as i32 / probe.width + probe_offset.1,
            );
            let b = &mut bounds[slot];
            b.0 = b.0.min(x);
            b.1 = b.1.min(y);
            b.2 = b.2.max(x + 1);
            b.3 = b.3.max(y + 1);
        }
    }
    let mut positions = Vec::new();
    let (mut left, mut top, mut right, mut bottom) = (0, 0, clean.width, clean.height);
    for (slot, &(x, y, r, b)) in slots.iter().zip(&bounds) {
        if x == i32::MAX
            || slot.marker_width > slot.advance
            || ((b - y) as f32 - slot.cell_height).abs() > 3.0
            || ((r - x) as f32 - slot.marker_width).abs() > 3.0
        {
            return Err(format!(
                "Could not locate ordered emoji slot: {:?}; expected {}x{}",
                (x, y, r, b),
                slot.marker_width,
                slot.cell_height
            ));
        }
        let x = (x as f32 + slot.offset_x - slot.image.origin_x).round() as i32;
        let y = (b as f32 - slot.cell_height * 0.2 - slot.image.baseline).round() as i32;
        left = left.min(x);
        top = top.min(y);
        right = right.max(x + slot.image.width as i32);
        bottom = bottom.max(y + slot.image.height as i32);
        positions.push((x, y));
    }
    let (w, h) = (right - left, bottom - top);
    if w > 8192 || h > 8192 || i64::from(w) * i64::from(h) > 16_777_216 {
        return Err("Emoji composite exceeds bitmap limit".into());
    }
    let mut bitmap = render::Bitmap {
        width: w,
        height: h,
        bgra: vec![0; (w * h * 4) as usize],
    };
    for y in 0..clean.height {
        let src = (y * clean.width * 4) as usize;
        let dst = (((y - top) * w - left) * 4) as usize;
        bitmap.bgra[dst..dst + clean.width as usize * 4]
            .copy_from_slice(&clean.bgra[src..src + clean.width as usize * 4]);
    }
    for (slot, (x, y)) in slots.iter().zip(positions) {
        for sy in 0..slot.image.height as i32 {
            for sx in 0..slot.image.width as i32 {
                let src = ((sy * slot.image.width as i32 + sx) * 4) as usize;
                let dst = (((sy + y - top) * w + sx + x - left) * 4) as usize;
                blend(
                    &mut bitmap.bgra[dst..dst + 4],
                    &slot.image.bgra[src..src + 4],
                );
            }
        }
    }
    Ok(Output {
        bitmap,
        cx: cx + (left + right - clean.width) as f32 / 2.0,
        cy: cy + (top + bottom - clean.height) as f32 / 2.0,
    })
}

fn blend(dst: &mut [u8], src: &[u8]) {
    let (a, old) = (u32::from(src[3]), u32::from(dst[3]));
    let total = a * 255 + old * (255 - a);
    if total == 0 {
        return;
    }
    for c in 0..3 {
        dst[c] =
            ((u32::from(src[c]) * a * 255 + u32::from(dst[c]) * old * (255 - a)) / total) as u8;
    }
    dst[3] = (total / 255) as u8;
}

#[cfg(test)]
pub(crate) fn verify_keycap_order(fonts: &mut dyn Fonts) {
    // Bare bases stay native; repeated fully/minimally qualified keycaps each
    // receive their own slot even when the font is COLR rather than SVG.
    let source = "0123456789 #* あ 1️⃣ 1\u{20E3} #️⃣ *️⃣ 1️⃣ 🔟 お";
    for family in ["Segoe UI Emoji", "Yu Gothic UI"] {
        let request = render::RenderRequest {
            text: source.into(),
            profile: None,
            font: family.into(),
            size: 32.0,
            spacing: 0.0,
            line_spacing: 0.0,
            color: "ffffff".into(),
            align: 0,
            bold: false,
            italic: false,
            disable_ligatures: false,
        };
        let styles = render::prepare_native(
            &request,
            source,
            &mut crate::font_coverage::UnknownGlyphCoverage,
        )
        .unwrap()
        .unwrap();
        let plan = build_plan(source, &request, &styles, fonts)
            .unwrap()
            .unwrap();
        assert_eq!(plan.slots.len(), 6);
        assert!(plan.probe.starts_with("0123456789 #* あ "));
        assert!(plan.clean.starts_with("0123456789 #* あ "));
        assert!(plan.clean.ends_with(" お"));
        assert_eq!(
            plan.slots
                .iter()
                .map(|s| s.color)
                .collect::<HashSet<_>>()
                .len(),
            6
        );
        let expected: Vec<_> = source
            .graphemes(true)
            .filter(|g| crate::keycap::is_keycap(g))
            .collect();
        for (cluster, slot) in expected.iter().zip(&plan.slots) {
            let expected = raster::glyph_with_fonts(fonts, cluster, family, 32.0).unwrap();
            assert_eq!(slot.image.bgra, expected.bgra, "{cluster}");
            assert_eq!(slot.image.svg_layers, 0, "Exercise the non-SVG keycap path");
            assert!(
                slot.advance >= slot.image.width as f32,
                "{cluster}: ink must fit the cell"
            );
        }
    }
}

#[cfg(test)]
pub(crate) fn verify_twitter_keycap_order(
    decorated: &str,
    request: &render::RenderRequest,
    styles: &crate::shaping::StyledText,
    fonts: &mut dyn Fonts,
) {
    let plan = build_plan(decorated, request, styles, fonts)
        .unwrap()
        .unwrap();
    assert_eq!(plan.slots.len(), 13);
    assert_eq!(
        plan.slots
            .iter()
            .map(|s| s.color)
            .collect::<HashSet<_>>()
            .len(),
        13
    );
    for slot in &plan.slots {
        assert_eq!(slot.image.svg_layers, 1);
        assert!(slot.advance > 0.0);
    }
    // Each letter-spacing slot uses its logical advance. SVG ink bounds and
    // origins must not be replaced with bitmap widths as in the mono fallback.
    let expected = raster::glyph_with_style(
        fonts,
        "0️⃣",
        &crate::shaping::TextStyle {
            family: "Twitter Color Emoji".into(),
            size: 32.0,
            horizontal: 2.0,
            vertical: 0.75,
            tracking: 0.0,
            baseline_shift: 0.0,
            color: 0xffffff,
            bold: false,
            italic: false,
            disable_ligatures: false,
        },
    )
    .unwrap();
    assert_eq!(plan.slots[0].image.bgra, expected.bgra);
    assert_eq!(plan.slots[0].advance, expected.advance);
    assert_eq!(plan.slots[0].image.origin_x, expected.origin_x);
}

#[cfg(test)]
pub(crate) fn verify_sample_order(fonts: &mut dyn Fonts) {
    let source = concat!(
        "🇯🇵🇺🇸🇬🇧🇯🇵 👩‍💻👍🏽\n",
        include_str!("../../../tests/aviutl2/emoji-mixed-sample.txt")
    );
    let mut profile = compositefont_core::CompositeFontProfile::neutral_default();
    let preset = compositefont_core::UNICODE_PRESETS
        .iter()
        .find(|p| p.block_name == "Emoji")
        .unwrap();
    profile
        .custom_ranges
        .push(preset.to_range(compositefont_core::FontAdjustment {
            font_family: "Twitter Color Emoji".into(),
            ..compositefont_core::FontAdjustment::neutral()
        }));
    let request = render::RenderRequest {
        text: source.into(),
        profile: Some(profile),
        font: "Yu Gothic UI".into(),
        size: 32.0,
        spacing: 0.0,
        line_spacing: 0.0,
        color: "ffffff".into(),
        align: 0,
        bold: false,
        italic: false,
        disable_ligatures: false,
    };
    let decorated = crate::decorate_plain_text_with_coverage(
        source,
        request.profile.as_ref().unwrap(),
        Some(request.size),
        Some(0.0),
        &mut crate::font_coverage::UnknownGlyphCoverage,
    );
    let native_request = render::RenderRequest {
        profile: None,
        text: decorated.clone(),
        ..request.clone()
    };
    let styles = render::prepare_native(
        &native_request,
        &decorated,
        &mut crate::font_coverage::UnknownGlyphCoverage,
    )
    .unwrap()
    .unwrap();
    let plan = build_plan(&decorated, &native_request, &styles, fonts)
        .unwrap()
        .unwrap();
    let emoji = icu_properties::CodePointSetData::new::<icu_properties::props::Emoji>();
    let expected: Vec<_> = source
        .graphemes(true)
        .filter(|g| emoji.contains(g.chars().next().unwrap()))
        .collect();
    assert_eq!(plan.slots.len(), expected.len());
    assert_eq!(plan.probe.matches(MARK).count(), expected.len());
    assert_eq!(plan.clean.matches(BLANK).count(), expected.len());
    assert_eq!(
        plan.probe.matches('\n').count(),
        source.matches('\n').count()
    );
    let mut cursor = 0;
    for (cluster, slot) in expected.iter().zip(&plan.slots) {
        let needle = format!("<#{:06x}>{MARK}", slot.color);
        let found = plan.probe[cursor..].find(&needle).unwrap();
        cursor += found + needle.len();
        let image = raster::glyph_with_fonts(fonts, cluster, "Twitter Color Emoji", 32.0).unwrap();
        assert_eq!(
            slot.image.bgra, image.bgra,
            "{cluster}: ordered slot image differs"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slot_controls_keep_both_axes_within_the_em_cell() {
        for (horizontal, vertical) in [
            (1.0, 1.0),
            (1.01, 1.0),
            (1.25, 1.0),
            (2.0, 1.0),
            (1.25, 0.75),
            (0.5, 1.5),
            (10.0, 1.0),
        ] {
            let (width, height) = (32.0 * horizontal, 32.0 * vertical);
            let (prefix, marker, _, native_advance) = slot_prefix(width, height, 0x070001);
            let source = format!("{prefix}{marker}");
            let parsed =
                parse_decorated_control_text(&source, ControlTextDefaults::default()).unwrap();
            let format = &parsed.spans[0].format;
            let em = format.effective_font_size().unwrap();
            let scale = &format.render_layout;
            assert!(scale.horizontal <= 1.0 && scale.vertical <= 1.0);
            let width_em = 1_u32 << ((marker as u32 - MARK as u32) / 2);
            assert!(
                (em * scale.horizontal * f64::from(width_em)
                    - f64::from(marker_width(width, height)))
                .abs()
                    < 0.05
            );
            assert!((em * scale.vertical - f64::from(height)).abs() < 0.05);
            assert!(
                (em * f64::from(width_em) + scale.tracking.unwrap() - f64::from(width)).abs()
                    < 0.05
            );
            assert!((native_advance as f64 - em * f64::from(width_em)).abs() < 0.05);
            assert!(
                (em - f64::from(height)).abs() < 0.001,
                "line-height increased for {horizontal}x{vertical}"
            );
        }
    }
    fn bitmap() -> render::Bitmap {
        render::Bitmap {
            width: 24,
            height: 10,
            bgra: vec![0; 24 * 10 * 4],
        }
    }
    fn slot(color: u32, pixel: [u8; 4]) -> Slot {
        Slot {
            color,
            offset_x: 0.0,
            cell_height: 10.0,
            advance: 6.0,
            marker_width: 6.0,
            image: raster::RasterImage {
                width: 1,
                height: 1,
                bgra: pixel.to_vec(),
                origin_x: 0.0,
                baseline: 8.0,
                advance: 6.0,
                svg_layers: 1,
            },
        }
    }
    fn rectangle(bitmap: &mut render::Bitmap, x: i32, color: u32) {
        for y in 0..10 {
            for x in x..x + 6 {
                let i = ((y * bitmap.width + x) * 4) as usize;
                bitmap.bgra[i..i + 4].copy_from_slice(&[
                    color as u8,
                    (color >> 8) as u8,
                    (color >> 16) as u8,
                    255,
                ]);
            }
        }
    }
    #[test]
    fn narrow_locators_preserve_variable_width_slots() {
        let clean = render::Bitmap {
            width: 100,
            height: 10,
            bgra: vec![0; 100 * 10 * 4],
        };
        let mut probe = render::Bitmap {
            width: 100,
            height: 10,
            bgra: vec![0; 100 * 10 * 4],
        };
        rectangle(&mut probe, 2, 0x070001);
        rectangle(&mut probe, 40, 0x070002);
        let mut a = slot(0x070001, [0, 0, 255, 255]);
        let mut b = slot(0x070002, [0, 255, 0, 255]);
        a.advance = 44.0;
        b.advance = 33.0;
        let output = compose(clean, &probe, &[a, b], 0.0, 0.0, (0, 0)).unwrap();
        assert_eq!(&output.bitmap.bgra[2 * 4..3 * 4], &[0, 0, 255, 255]);
        assert_eq!(&output.bitmap.bgra[40 * 4..41 * 4], &[0, 255, 0, 255]);
    }

    #[test]
    fn marker_spacing_correction_preserves_the_svg_origin() {
        let mut clean = bitmap();
        clean.bgra[..4].copy_from_slice(&[80, 90, 100, 128]);
        let mut probe = bitmap();
        rectangle(&mut probe, 2, 0x070001);
        let mut value = slot(0x070001, [0, 0, 255, 255]);
        value.offset_x = 12.0;
        let output = compose(clean, &probe, &[value], 0.0, 0.0, (0, 0)).unwrap();
        assert_eq!(&output.bitmap.bgra[..4], &[80, 90, 100, 128]);
        assert_eq!(&output.bitmap.bgra[14 * 4..15 * 4], &[0, 0, 255, 255]);
        assert_eq!(&output.bitmap.bgra[2 * 4..3 * 4], &[0, 0, 0, 0]);
    }

    #[test]
    fn ordered_slots_use_ids_and_keep_native_pixels() {
        let mut clean = bitmap();
        clean.bgra[..4].copy_from_slice(&[80, 90, 100, 128]);
        let mut probe = bitmap();
        rectangle(&mut probe, 14, 0x070001);
        rectangle(&mut probe, 2, 0x080002);
        let output = compose(
            clean,
            &probe,
            &[
                slot(0x070001, [0, 0, 255, 255]),
                slot(0x080002, [255, 0, 0, 255]),
            ],
            -12.0,
            -5.0,
            (0, 0),
        )
        .unwrap();
        assert_eq!(&output.bitmap.bgra[..4], &[80, 90, 100, 128]);
        assert_eq!(&output.bitmap.bgra[14 * 4..15 * 4], &[0, 0, 255, 255]);
        assert_eq!(&output.bitmap.bgra[2 * 4..3 * 4], &[255, 0, 0, 255]);
        assert_eq!((output.cx, output.cy), (-12.0, -5.0));
        assert!(
            output
                .bitmap
                .bgra
                .chunks_exact(4)
                .enumerate()
                .all(|(i, p)| [0, 2, 14].contains(&i) || p == [0, 0, 0, 0])
        );
    }
    #[test]
    fn one_pixel_ink_crop_uses_unchanged_text_to_align_slots() {
        let mut clean = bitmap();
        clean.bgra[..4].copy_from_slice(&[80, 90, 100, 128]);
        let mut probe = bitmap();
        probe.width += 1;
        probe
            .bgra
            .resize((probe.width * probe.height * 4) as usize, 0);
        probe.bgra[..4].copy_from_slice(&clean.bgra[..4]);
        rectangle(&mut probe, 14, 0x070001);
        let slots = [slot(0x070001, [0, 0, 255, 255])];
        assert_eq!(
            align_native_images(&probe, &clean, &slots, -0.5, 0.0).unwrap(),
            (0, 0)
        );
        clean.bgra[0] += 1;
        assert!(align_native_images(&probe, &clean, &slots, -0.5, 0.0).is_err());
    }

    #[test]
    fn different_crop_origins_translate_slot_positions() {
        let mut clean = bitmap();
        clean.bgra[4..8].copy_from_slice(&[80, 90, 100, 128]);
        let mut probe = bitmap();
        probe.bgra[..4].copy_from_slice(&clean.bgra[4..8]);
        rectangle(&mut probe, 14, 0x070001);
        let slots = [slot(0x070001, [0, 0, 255, 255])];
        let offset = align_native_images(&probe, &clean, &slots, 1.0, 0.0).unwrap();
        assert_eq!(offset, (1, 0));
        let output = compose(clean, &probe, &slots, 0.0, 0.0, offset).unwrap();
        assert_eq!(&output.bitmap.bgra[15 * 4..16 * 4], &[0, 0, 255, 255]);
    }
    #[test]
    fn missing_or_rescaled_marker_rejects_the_frame() {
        assert!(
            compose(
                bitmap(),
                &bitmap(),
                &[slot(0x070001, [0, 0, 255, 255])],
                0.0,
                0.0,
                (0, 0)
            )
            .is_err()
        );
        let mut probe = bitmap();
        rectangle(&mut probe, 2, 0x070001);
        let mut value = slot(0x070001, [0, 0, 255, 255]);
        value.cell_height = 20.0;
        assert!(compose(bitmap(), &probe, &[value], 0.0, 0.0, (0, 0)).is_err());
    }
}
