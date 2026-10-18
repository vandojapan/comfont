use eframe::egui;
use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use ttf_parser::{Face, Tag, name_id};

const PREVIEW_FALLBACK_FAMILY: &str = "Yu Gothic UI";

#[derive(Clone)]
struct FontFaceFile {
    path: PathBuf,
    index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FontVariation {
    pub label: String,
    pub family: String,
}

#[derive(Clone, Debug)]
struct FaceVariation {
    label: String,
    weight: u16,
    font_names: Vec<String>,
}

#[derive(Default)]
struct FontCatalog {
    faces: HashMap<String, FontFaceFile>,
    variations: HashMap<String, Vec<FaceVariation>>,
}

#[derive(Default)]
struct CatalogState {
    started: bool,
    faces: Option<HashMap<String, FontFaceFile>>,
    variations: Option<HashMap<String, Vec<FaceVariation>>>,
    loaded: HashMap<String, Arc<egui::FontData>>,
    preview_text_support: HashMap<String, bool>,
    loading: HashSet<String>,
    installed: HashSet<String>,
    fallback_textures: HashMap<String, egui::TextureHandle>,
}

#[derive(Clone, Default)]
pub(super) struct FontPreviewCatalog {
    state: Arc<Mutex<CatalogState>>,
}

impl FontPreviewCatalog {
    pub(super) fn variations(
        &self,
        family: &str,
        host_fonts: &[String],
        ctx: &egui::Context,
    ) -> Option<Vec<FontVariation>> {
        self.ensure_scan_started(ctx);
        let state = self.state.try_lock().ok()?;
        let candidates = state.variations.as_ref()?.get(&normalize_name(family))?;
        resolve_host_variations(candidates, host_fonts)
    }

    pub(super) fn preview(
        &self,
        family: &str,
        font_size: f32,
        ctx: &egui::Context,
    ) -> (Option<String>, Option<egui::TextureHandle>) {
        let key = normalize_name(family);
        let use_fallback = self
            .state
            .try_lock()
            .ok()
            .and_then(|state| state.preview_text_support.get(&key).copied())
            == Some(false);
        let display_family = if use_fallback {
            PREVIEW_FALLBACK_FAMILY
        } else {
            family
        };
        let family_tag = self.family_tag(display_family, ctx);
        let texture = if family_tag.is_none() {
            self.fallback_texture(display_family, font_size, ctx)
        } else {
            None
        };
        (family_tag, texture)
    }

    pub(super) fn family_tag(&self, family: &str, ctx: &egui::Context) -> Option<String> {
        let key = normalize_name(family);
        if key.is_empty() {
            return None;
        }
        self.ensure_scan_started(ctx);
        let mut state = self.state.try_lock().ok()?;
        if !state.installed.contains(&key)
            && let Some(data) = state.loaded.get(&key).cloned()
        {
            let tag = font_tag(&key);
            ctx.add_font(egui::epaint::text::FontInsert::new(
                &tag,
                (*data).clone(),
                vec![egui::epaint::text::InsertFontFamily {
                    family: egui::FontFamily::Name(tag.clone().into()),
                    priority: egui::epaint::text::FontPriority::Highest,
                }],
            ));
            state.installed.insert(key.clone());
        }
        if state.installed.contains(&key) {
            // add_font queues the binding for the next egui pass. The picker
            // can ask for the same face more than once before that pass runs.
            let tag = font_tag(&key);
            let family = egui::FontFamily::Name(tag.clone().into());
            if ctx.fonts(|fonts| fonts.families().contains(&family)) {
                return Some(tag);
            }
            ctx.request_repaint();
            return None;
        }
        let face = state.faces.as_ref()?.get(&key)?.clone();
        if state.loading.insert(key.clone()) {
            let shared = Arc::clone(&self.state);
            let repaint = ctx.clone();
            let preview_text = family.to_owned();
            let worker_key = key.clone();
            std::thread::spawn(move || {
                let data = load_font_data(&face);
                let supports_preview_text = data
                    .as_ref()
                    .is_some_and(|data| font_supports_preview_text(data, &preview_text));
                if let Ok(mut state) = shared.lock() {
                    state.loading.remove(&worker_key);
                    state
                        .preview_text_support
                        .insert(worker_key.clone(), supports_preview_text);
                    if let Some(data) = data {
                        state.loaded.insert(worker_key, Arc::new(data));
                    }
                }
                repaint.request_repaint();
            });
        }
        None
    }

    fn ensure_scan_started(&self, ctx: &egui::Context) {
        let Ok(mut state) = self.state.try_lock() else {
            return;
        };
        if state.started {
            return;
        }
        state.started = true;
        drop(state);

        let shared = Arc::clone(&self.state);
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            // Enumeration and font parsing may take seconds. Never hold the UI
            // cache lock during this work.
            let catalog = scan_font_catalog();
            if let Ok(mut state) = shared.lock() {
                state.faces = Some(catalog.faces);
                state.variations = Some(catalog.variations);
            }
            repaint.request_repaint();
        });
    }

    fn fallback_texture(
        &self,
        family: &str,
        font_size: f32,
        ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        let pixels_per_point = ctx.pixels_per_point().max(1.0);
        let font_height = (font_size * pixels_per_point).round().clamp(1.0, 128.0) as i32;
        let key = format!("{}@{font_height}", normalize_name(family));
        if let Some(texture) = self.state.try_lock().ok()?.fallback_textures.get(&key) {
            return Some(texture.clone());
        }
        let image = gdi_preview_image(family, font_height)?;
        let texture = ctx.load_texture(
            format!("ComfontPreviewText/{key}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.state
            .try_lock()
            .ok()?
            .fallback_textures
            .insert(key, texture.clone());
        Some(texture)
    }
}

#[repr(C)]
struct DibHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pixels_per_meter: i32,
    y_pixels_per_meter: i32,
    colors_used: u32,
    colors_important: u32,
}

#[repr(C)]
struct DibInfo {
    header: DibHeader,
    colors: [u32; 1],
}

#[repr(C)]
#[derive(Default)]
struct GdiSize {
    width: i32,
    height: i32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn AddFontResourceExW(filename: *const u16, flags: u32, reserved: *mut c_void) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike_out: u32,
        char_set: u32,
        output_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face_name: *const u16,
    ) -> *mut c_void;
    fn CreateDIBSection(
        dc: *mut c_void,
        info: *const DibInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> *mut c_void;
    fn SelectObject(dc: *mut c_void, object: *mut c_void) -> *mut c_void;
    fn DeleteObject(object: *mut c_void) -> i32;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn SetBkMode(dc: *mut c_void, mode: i32) -> i32;
    fn SetTextColor(dc: *mut c_void, color: u32) -> u32;
    fn GetTextExtentPoint32W(
        dc: *mut c_void,
        text: *const u16,
        length: i32,
        size: *mut GdiSize,
    ) -> i32;
    fn TextOutW(dc: *mut c_void, x: i32, y: i32, text: *const u16, length: i32) -> i32;
}

fn gdi_preview_image(family: &str, font_height: i32) -> Option<egui::ColorImage> {
    let text: Vec<u16> = family.encode_utf16().collect();
    if text.is_empty() {
        return None;
    }
    let dc = unsafe { CreateCompatibleDC(std::ptr::null_mut()) };
    if dc.is_null() {
        return None;
    }
    let mut face: Vec<u16> = family.encode_utf16().collect();
    face.push(0);
    let font = unsafe {
        CreateFontW(
            -font_height,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1, // DEFAULT_CHARSET
            0,
            0,
            4, // ANTIALIASED_QUALITY
            0,
            face.as_ptr(),
        )
    };
    if font.is_null() {
        unsafe { DeleteDC(dc) };
        return None;
    }
    let old_font = unsafe { SelectObject(dc, font) };
    let mut extent = GdiSize::default();
    let measured =
        unsafe { GetTextExtentPoint32W(dc, text.as_ptr(), text.len() as i32, &mut extent) != 0 };
    if !measured || extent.width <= 0 {
        unsafe {
            SelectObject(dc, old_font);
            DeleteObject(font);
            DeleteDC(dc);
        }
        return None;
    }
    let width = (extent.width + 4).clamp(1, 1024);
    let height = (extent.height + 4).clamp(1, 128);
    let info = DibInfo {
        header: DibHeader {
            size: std::mem::size_of::<DibHeader>() as u32,
            width,
            height: -height,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_pixels_per_meter: 0,
            y_pixels_per_meter: 0,
            colors_used: 0,
            colors_important: 0,
        },
        colors: [0],
    };
    let mut bits = std::ptr::null_mut();
    let bitmap = unsafe { CreateDIBSection(dc, &info, 0, &mut bits, std::ptr::null_mut(), 0) };
    if bitmap.is_null() || bits.is_null() {
        unsafe {
            SelectObject(dc, old_font);
            DeleteObject(font);
            DeleteDC(dc);
        }
        return None;
    }
    let old_bitmap = unsafe { SelectObject(dc, bitmap) };
    let byte_count = (width * height * 4) as usize;
    unsafe {
        bits.cast::<u8>().write_bytes(255, byte_count);
        SetBkMode(dc, 1); // TRANSPARENT
        SetTextColor(dc, 0);
        TextOutW(dc, 2, 2, text.as_ptr(), text.len() as i32);
    }
    let source = unsafe { std::slice::from_raw_parts(bits.cast::<u8>(), byte_count) };
    let pixels = source
        .chunks_exact(4)
        .map(|pixel| {
            let luminance = (u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2])) / 3;
            egui::Color32::from_white_alpha(255 - luminance as u8)
        })
        .collect();
    unsafe {
        SelectObject(dc, old_bitmap);
        SelectObject(dc, old_font);
        DeleteObject(bitmap);
        DeleteObject(font);
        DeleteDC(dc);
    }
    Some(egui::ColorImage::new(
        [width as usize, height as usize],
        pixels,
    ))
}

fn font_tag(key: &str) -> String {
    format!("ComfontPreview/{key}")
}

fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

fn font_supports_preview_text(data: &egui::FontData, text: &str) -> bool {
    let Ok(face) = Face::parse(data.as_ref(), data.index) else {
        return false;
    };
    text.chars()
        .filter(|character| !character.is_whitespace())
        .all(|character| {
            face.glyph_index(character)
                .is_some_and(|glyph| face.glyph_bounding_box(glyph).is_some())
        })
}

fn load_font_data(face: &FontFaceFile) -> Option<egui::FontData> {
    let bytes = std::fs::read(&face.path).ok()?;
    let font = egui::FontData {
        index: face.index,
        ..egui::FontData::from_owned(bytes)
    };
    Some(font)
}

fn scan_font_catalog() -> FontCatalog {
    static REGISTERED_PRIVATE_FONTS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    let private_font_root = aviutl2::config::app_data_path().join("Font");
    let mut directories = Vec::new();
    if let Some(windows) = std::env::var_os("WINDIR") {
        directories.push(PathBuf::from(windows).join("Fonts"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(local).join("Microsoft/Windows/Fonts"));
    }
    directories.push(private_font_root.clone());

    let mut files = Vec::new();
    for directory in directories {
        collect_font_files(&directory, &mut files);
    }
    files.sort();
    files.dedup();

    let mut faces = HashMap::new();
    let mut family_variations = HashMap::<String, Vec<FaceVariation>>::new();
    let mut aliases = HashMap::<String, String>::new();
    for path in files {
        if path.starts_with(&private_font_root) {
            let registered = REGISTERED_PRIVATE_FONTS.get_or_init(Default::default);
            if let Ok(mut registered) = registered.lock()
                && registered.insert(path.clone())
            {
                let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
                wide.push(0);
                // Make app-private fonts visible to the UI's GDI fallback preview.
                if unsafe { AddFontResourceExW(wide.as_ptr(), 0x10, std::ptr::null_mut()) } == 0 {
                    registered.remove(&path);
                }
            }
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let count = ttf_parser::fonts_in_collection(&bytes).unwrap_or(1);
        for index in 0..count {
            let Ok(face) = Face::parse(&bytes, index) else {
                continue;
            };
            let mut family_names = Vec::new();
            let mut typographic_families = Vec::new();
            let mut subfamilies = Vec::new();
            let mut full_names = Vec::new();
            for name in face.names() {
                let Some(value) = name.to_string() else {
                    continue;
                };
                match name.name_id {
                    name_id::FAMILY => family_names.push(value),
                    name_id::TYPOGRAPHIC_FAMILY => typographic_families.push(value),
                    name_id::SUBFAMILY | name_id::TYPOGRAPHIC_SUBFAMILY => subfamilies.push(value),
                    name_id::FULL_NAME => full_names.push(value),
                    _ => {}
                }
            }
            let canonical_family = typographic_families
                .first()
                .or_else(|| family_names.first())
                .cloned();
            if let Some(canonical_family) = canonical_family {
                let canonical_key = normalize_name(&canonical_family);
                if !canonical_key.is_empty() {
                    let options =
                        if face.is_italic() || face.is_oblique() || face.width().to_number() != 5 {
                            Vec::new()
                        } else {
                            variations_for_face(
                                &face,
                                &family_names,
                                &typographic_families,
                                &subfamilies,
                                &full_names,
                            )
                        };
                    let group = family_variations.entry(canonical_key.clone()).or_default();
                    for option in options {
                        merge_face_variation(group, option);
                    }
                    for alias in family_names
                        .into_iter()
                        .chain(typographic_families)
                        .chain(full_names)
                    {
                        let alias = normalize_name(&alias);
                        if !alias.is_empty() {
                            aliases
                                .entry(alias)
                                .or_insert_with(|| canonical_key.clone());
                        }
                    }
                }
            }
            // egui's glyph rasterizer accepts TrueType outlines. CFF/color
            // faces use the cached GDI text texture rendered for this font name.
            if face.tables().glyf.is_none() {
                continue;
            }
            for family in face
                .names()
                .into_iter()
                .filter(|name| {
                    matches!(
                        name.name_id,
                        name_id::FAMILY | name_id::TYPOGRAPHIC_FAMILY | name_id::FULL_NAME
                    )
                })
                .filter_map(|name| name.to_string())
            {
                let key = normalize_name(&family);
                if !key.is_empty() {
                    faces.entry(key).or_insert_with(|| FontFaceFile {
                        path: path.clone(),
                        index,
                    });
                }
            }
            let preview_families = face
                .names()
                .into_iter()
                .filter(|name| {
                    name.name_id == name_id::FAMILY || name.name_id == name_id::TYPOGRAPHIC_FAMILY
                })
                .filter_map(|name| name.to_string())
                .collect::<Vec<_>>();
            let preview_styles = face
                .names()
                .into_iter()
                .filter(|name| {
                    name.name_id == name_id::SUBFAMILY
                        || name.name_id == name_id::TYPOGRAPHIC_SUBFAMILY
                })
                .filter_map(|name| name.to_string())
                .collect::<Vec<_>>();
            for alias in preview_families.iter().flat_map(|family| {
                preview_styles
                    .iter()
                    .map(move |style| format!("{family} {style}"))
            }) {
                let key = normalize_name(&alias);
                if !key.is_empty() {
                    faces.entry(key).or_insert_with(|| FontFaceFile {
                        path: path.clone(),
                        index,
                    });
                }
            }
        }
    }
    let variations = aliases
        .into_iter()
        .filter_map(|(alias, canonical)| {
            let mut options = family_variations.get(&canonical)?.clone();
            options.sort_by_key(|option| option.weight);
            options.dedup_by_key(|option| option.weight);
            (options.len() > 1).then_some((alias, options))
        })
        .collect();
    FontCatalog { faces, variations }
}

fn variations_for_face(
    face: &Face<'_>,
    family_names: &[String],
    typographic_families: &[String],
    subfamilies: &[String],
    full_names: &[String],
) -> Vec<FaceVariation> {
    let families = typographic_families
        .iter()
        .chain(family_names)
        .collect::<Vec<_>>();
    let weight_axis = face
        .variation_axes()
        .into_iter()
        .find(|axis| axis.tag == Tag::from_bytes(b"wght"));
    if let Some(axis) = weight_axis.filter(|axis| axis.max_value > axis.min_value) {
        let mut weights = (100..=900)
            .step_by(100)
            .filter(|weight| {
                (*weight as f32) >= axis.min_value && (*weight as f32) <= axis.max_value
            })
            .collect::<Vec<_>>();
        let default = axis.def_value.round().clamp(1.0, 999.0) as u16;
        weights.push(default);
        weights.sort_unstable();
        weights.dedup();
        return weights
            .into_iter()
            .map(|weight| {
                let mut font_names = families
                    .iter()
                    .flat_map(|family| {
                        [
                            format!("{} {}", family, standard_weight_name(weight)),
                            format!("{} {}", family, weight),
                        ]
                    })
                    .chain(full_names.iter().cloned())
                    .collect::<Vec<_>>();
                if weight == default {
                    font_names.extend(families.iter().map(|family| family.to_string()));
                }
                dedup_font_names(&mut font_names);
                FaceVariation {
                    label: standard_weight_label(weight),
                    weight,
                    font_names,
                }
            })
            .collect();
    }

    let weight = face.weight().to_number().clamp(1, 999);
    let label = subfamilies
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
        .min_by_key(|name| subfamily_label_rank(name))
        .map(str::to_owned)
        .unwrap_or_else(|| standard_weight_label(weight));
    let mut font_names = full_names.to_vec();
    font_names.extend(families.iter().map(|family| format!("{family} {label}")));
    font_names.extend(families.into_iter().cloned());
    dedup_font_names(&mut font_names);
    vec![FaceVariation {
        label,
        weight,
        font_names,
    }]
}

fn merge_face_variation(group: &mut Vec<FaceVariation>, mut new: FaceVariation) {
    if let Some(existing) = group.iter_mut().find(|item| item.weight == new.weight) {
        if subfamily_label_rank(&new.label) < subfamily_label_rank(&existing.label) {
            existing.label = new.label;
        }
        existing.font_names.append(&mut new.font_names);
        dedup_font_names(&mut existing.font_names);
    } else {
        group.push(new);
    }
}

fn dedup_font_names(names: &mut Vec<String>) {
    let mut seen = HashSet::new();
    names.retain(|name| seen.insert(normalize_name(name)));
}

fn resolve_host_variations(
    candidates: &[FaceVariation],
    host_fonts: &[String],
) -> Option<Vec<FontVariation>> {
    let host_fonts = host_fonts
        .iter()
        .filter(|font| is_safe_font_name(font))
        .map(|font| (normalize_name(font), font.as_str()))
        .collect::<HashMap<_, _>>();
    let mut seen_families = HashSet::new();
    let variations = candidates
        .iter()
        .filter_map(|variation| {
            let font = variation
                .font_names
                .iter()
                .find_map(|name| host_fonts.get(&normalize_name(name)).copied())?;
            seen_families
                .insert(normalize_name(font))
                .then(|| FontVariation {
                    label: variation.label.clone(),
                    family: font.to_owned(),
                })
        })
        .collect::<Vec<_>>();
    (variations.len() > 1).then_some(variations)
}

fn is_safe_font_name(name: &str) -> bool {
    !name.trim().is_empty()
        && !name
            .chars()
            .any(|character| matches!(character, '<' | '>' | ',') || character.is_control())
}

fn subfamily_label_rank(name: &str) -> (usize, usize) {
    let name = name.trim();
    let chars = name.chars().count();
    let common_abbreviation = matches!(
        name.to_ascii_uppercase().as_str(),
        "B" | "H" | "M" | "R" | "L" | "DB" | "EB"
    );
    (if common_abbreviation { 0 } else { 1 }, chars)
}

fn standard_weight_label(weight: u16) -> String {
    format!("{} ({weight})", standard_weight_name(weight))
}

fn standard_weight_name(weight: u16) -> &'static str {
    match weight {
        1..=149 => "Thin",
        150..=249 => "Extra Light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semi Bold",
        650..=749 => "Bold",
        750..=849 => "Extra Bold",
        _ => "Black",
    }
}

fn collect_font_files(directory: &Path, output: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_font_files(&path, output);
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "ttf" | "ttc" | "otf" | "otc"
                    )
                })
        {
            output.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_distinct_open_type_styles_only_when_the_host_lists_them() {
        let candidates = vec![
            FaceVariation {
                label: "R".into(),
                weight: 400,
                font_names: vec!["Noto Sans JP Regular".into(), "Noto Sans JP".into()],
            },
            FaceVariation {
                label: "B".into(),
                weight: 700,
                font_names: vec!["Noto Sans JP B".into(), "Noto Sans JP Bold".into()],
            },
            FaceVariation {
                label: "H".into(),
                weight: 900,
                font_names: vec!["Noto Sans JP H".into(), "Noto Sans JP Heavy".into()],
            },
        ];
        let host_fonts = vec![
            "Noto Sans JP".into(),
            "Noto Sans JP B".into(),
            "Noto Sans JP H".into(),
        ];

        assert_eq!(
            resolve_host_variations(&candidates, &host_fonts),
            Some(vec![
                FontVariation {
                    label: "R".into(),
                    family: "Noto Sans JP".into(),
                },
                FontVariation {
                    label: "B".into(),
                    family: "Noto Sans JP B".into(),
                },
                FontVariation {
                    label: "H".into(),
                    family: "Noto Sans JP H".into(),
                },
            ])
        );
        assert!(resolve_host_variations(&candidates, &["Noto Sans JP".into()]).is_none());
    }

    #[test]
    fn abbreviation_labels_keep_b_and_h_style_names() {
        assert!(subfamily_label_rank("B") < subfamily_label_rank("Bold"));
        assert!(subfamily_label_rank("H") < subfamily_label_rank("Heavy"));
    }

    #[test]
    fn queued_font_is_used_only_after_egui_binds_the_family() {
        let catalog = FontPreviewCatalog::default();
        let data = egui::FontDefinitions::default()
            .font_data
            .values()
            .next()
            .unwrap()
            .clone();
        {
            let mut state = catalog.state.lock().unwrap();
            state.started = true;
            state.loaded.insert(normalize_name("Test Face"), data);
        }
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            assert!(catalog.family_tag("Test Face", ctx).is_none());
            assert!(catalog.family_tag("Test Face", ctx).is_none());
        });
        let _ = ctx.run(Default::default(), |ctx| {
            let tag = catalog.family_tag("Test Face", ctx).unwrap();
            ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap(
                    "Test Face".to_owned(),
                    egui::FontId::new(16.0, egui::FontFamily::Name(tag.into())),
                    egui::Color32::WHITE,
                );
            });
        });
    }

    #[test]
    fn rendering_never_waits_for_the_font_catalog_worker() {
        let catalog = FontPreviewCatalog::default();
        let _worker_lock = catalog.state.lock().unwrap();
        let (family, texture) = catalog.preview("Arial", 20.0, &egui::Context::default());
        assert!(family.is_none() && texture.is_none());
    }

    #[test]
    fn gdi_fallback_renders_the_family_name_as_a_transparent_texture() {
        let image = gdi_preview_image("Arial", 18).expect("GDI can resolve a font family");
        assert!(image.size[0] > 20 && image.size[1] > 18);
        assert!(image.pixels.iter().any(|pixel| pixel.a() > 0));
        assert!(image.pixels.iter().any(|pixel| pixel.a() < 255));
    }

    #[test]
    fn preview_coverage_ignores_spaces_and_detects_missing_glyphs() {
        let definitions = egui::FontDefinitions::default();
        let data = definitions.font_data.values().next().unwrap();
        assert!(font_supports_preview_text(data, "A B"));
        assert!(font_supports_preview_text(data, "   "));
        assert!(!font_supports_preview_text(data, "\u{10FFFF}"));
    }
}
