use crate::{FontManagerGlyphCoverage, ReloadableProfiles, render, shaping, supports_text};
use aviutl2::{
    AnyResult,
    filter::{
        FilterConfigItem, FilterPlugin, FilterPluginFlags, FilterPluginTable, FilterProcVideo,
        ImageResource, InputImageResourcePixelFormat,
    },
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const PROFILE_EFFECT: &str = "描画@comfont";
const PROFILE_ITEM: &str = "comfont::プロファイル";
const FORCE_LIGATURES_ITEM: &str = "comfont::リガチャを強制ON";
const ANTIALIAS_ITEM: &str = "comfont::アンチエイリアス";
const NATIVE_EFFECT: &str = "Comfont native text";
const MAX_TEXT_BYTES: usize = 64 * 1024;
static DIAGNOSTIC_EVENTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn log_once(event: u32, message: &str) {
    let mask = 1_u64 << event;
    if DIAGNOSTIC_EVENTS.fetch_or(mask, std::sync::atomic::Ordering::Relaxed) & mask == 0 {
        let _ = aviutl2::logger::write_info_log(&format!("Comfont native text: {message}"));
    }
}

/// Re-runs AviUtl2's native text input with transiently decorated parameters.
/// The stored object alias is only read; its text value is never written back.
#[aviutl2::plugin(FilterPlugin)]
pub struct NativeTextFilter {
    profiles: Mutex<ReloadableProfiles>,
    emoji_cache: Mutex<crate::emoji_bypass::Cache>,
    ligature_cache: Mutex<
        VecDeque<(
            render::RenderRequest,
            shaping::AntialiasMethod,
            Arc<render::Bitmap>,
        )>,
    >,
}

impl FilterPlugin for NativeTextFilter {
    type Userdata = ();

    fn new(_info: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        Ok(Self {
            profiles: Mutex::new(ReloadableProfiles::new(super::profile_path())),
            emoji_cache: Mutex::new(crate::emoji_bypass::Cache::default()),
            ligature_cache: Mutex::new(VecDeque::new()),
        })
    }

    fn plugin_info(&self) -> FilterPluginTable {
        FilterPluginTable {
            name: NATIVE_EFFECT.to_owned(),
            label: None,
            information: "Comfont's native text renderer".to_owned(),
            flags: aviutl2::bitflag!(FilterPluginFlags {
                video: true,
                hidemenu: true,
            }),
            config_items: Vec::new(),
        }
    }

    fn proc_video(
        &self,
        _config: &[FilterConfigItem],
        video: &mut FilterProcVideo<()>,
    ) -> AnyResult<()> {
        let started = std::time::Instant::now();
        log_once(0, "renderer entered");
        let object = video.object.handle;
        let section = video.read_section();
        let count = match section.count_object_effect(object, PROFILE_EFFECT) {
            Ok(count) => count,
            Err(error) => {
                log_once(1, &format!("could not count profile effects: {error}"));
                return Ok(());
            }
        };
        if count != 1 {
            log_once(
                2,
                &format!("expected one {PROFILE_EFFECT} effect, found {count}"),
            );
            return Ok(());
        }
        let profile = match section.get_object_effect_item(object, PROFILE_EFFECT, 0, PROFILE_ITEM)
        {
            Ok(profile) => profile,
            Err(error) => {
                log_once(3, &format!("could not read the selected profile: {error}"));
                return Ok(());
            }
        };
        if profile.len() > 256 {
            log_once(4, "profile name exceeded 256 bytes");
            return Ok(());
        }

        let object_alias = match section.get_object_alias(object) {
            Ok(alias) => alias,
            Err(error) => {
                log_once(5, &format!("could not read the object alias: {error}"));
                return Ok(());
            }
        };
        let Some(mut params) = native_text_parameters(&object_alias) else {
            log_once(
                6,
                "could not find exactly one standard text effect in the object alias",
            );
            return Ok(());
        };
        let Some(source_alias) = params
            .iter()
            .find(|(key, _)| key == "テキスト")
            .map(|(_, value)| value.clone())
        else {
            log_once(7, "standard text effect has no テキスト value");
            return Ok(());
        };
        let source = render::decode_alias(&source_alias);
        if source.len() > MAX_TEXT_BYTES {
            log_once(8, "source text exceeded 64 KiB");
            return Ok(());
        }
        if source.contains('\0') {
            log_once(9, "source text contains NUL");
            return Ok(());
        }
        if source.contains("<?") {
            log_once(10, "Lua text is left to manual integration");
            return Ok(());
        }
        if !supports_text(&source) {
            log_once(11, "source text contains unsupported controls");
            return Ok(());
        }
        let base_size = parameter_number(&params, "サイズ");
        let base_spacing = parameter_number(&params, "字間");
        if base_size.is_none() {
            log_once(12, "standard text size could not be read");
            return Ok(());
        }
        let profile = if profile.is_empty() {
            compositefont_core::DEFAULT_PROFILE_NAME.to_owned()
        } else {
            profile
        };

        let force_ligatures = section
            .get_object_effect_item(object, PROFILE_EFFECT, 0, FORCE_LIGATURES_ITEM)
            .is_ok_and(|value| is_checked(&value));
        let antialias = section
            .get_object_effect_item(object, PROFILE_EFFECT, 0, ANTIALIAS_ITEM)
            .ok()
            .as_deref()
            .map(parse_antialias)
            .unwrap_or_default();
        let settings_elapsed = started.elapsed();
        log_once(22, "object settings read");
        if force_ligatures && self.render_ligatures(video, &params, &source, &profile, antialias) {
            return Ok(());
        }

        let decorated = {
            let section = video.read_section();
            let mut coverage = FontManagerGlyphCoverage::new(section);
            super::decorate_host_text(source.clone(), false, |text| {
                decorate_native_text(
                    &self.profiles,
                    &text,
                    &profile,
                    base_size,
                    base_spacing,
                    &mut coverage,
                )
            })
        };
        let decoration_elapsed = started.elapsed();
        if self.render_svg_emoji(video, &params, &decorated) {
            return Ok(());
        }
        if decorated == source || decorated.len() > MAX_TEXT_BYTES {
            log_once(
                14,
                &format!(
                    "no decorated output produced (profile selected, source {} bytes)",
                    source.len()
                ),
            );
            return Ok(());
        }

        let Some((_, text_value)) = params.iter_mut().find(|(key, _)| key == "テキスト") else {
            return Ok(());
        };
        *text_value = encode_alias(&decorated);
        if params
            .iter()
            .any(|(key, value)| key.contains('\0') || value.contains('\0'))
        {
            log_once(15, "a generated text parameter contains NUL");
            return Ok(());
        }

        log_once(
            16,
            &format!(
                "decorated text prepared (profile selected, {} -> {} bytes)",
                source.len(),
                decorated.len()
            ),
        );
        // This host call invokes the standard DirectWrite text renderer with
        // temporary values. Only the rendered image changes for this frame.
        match video.exec_effect(
            "テキスト",
            params.iter().map(|(key, value)| (key, value)),
            &ImageResource::Object,
        ) {
            Ok(()) => log_once(17, "standard text renderer completed successfully"),
            Err(error) => {
                log_once(18, &format!("standard text renderer failed: {error}"));
            }
        }
        let total = started.elapsed();
        if total >= std::time::Duration::from_millis(100) {
            log_once(
                23,
                &format!(
                    "slow frame: settings {:.1} ms, decoration {:.1} ms, native text {:.1} ms, total {:.1} ms",
                    settings_elapsed.as_secs_f64() * 1000.0,
                    (decoration_elapsed - settings_elapsed).as_secs_f64() * 1000.0,
                    (total - decoration_elapsed).as_secs_f64() * 1000.0,
                    total.as_secs_f64() * 1000.0,
                ),
            );
        }
        Ok(())
    }
}

fn decorate_native_text(
    shared: &Mutex<ReloadableProfiles>,
    source: &str,
    profile_name: &str,
    size: Option<f64>,
    spacing: Option<f64>,
    coverage: &mut dyn super::GlyphCoverage,
) -> String {
    let profile = {
        let Ok(mut profiles) = shared.lock() else {
            log_once(13, "profile lock was poisoned");
            return source.to_owned();
        };
        profiles.refresh_if_changed();
        let Ok(profile) = profiles.store.profile(Some(profile_name)) else {
            return source.to_owned();
        };
        let profile = profile.clone();
        if let Some(result) = profiles.cached_decoration(source, &profile, size, spacing) {
            return result;
        }
        profile
    };
    // FontManager calls can enter host code. Never hold our profile/cache lock
    // while making those calls or serializing other rendering threads behind them.
    let result =
        super::decorate_plain_text_with_coverage(source, &profile, size, spacing, coverage);
    if let Ok(mut profiles) = shared.lock() {
        profiles.cache_decoration(super::DecoratedTextCacheEntry {
            source: source.to_owned(),
            profile,
            size,
            spacing,
            result: result.clone(),
        });
    }
    result
}

impl NativeTextFilter {
    fn render_svg_emoji(
        &self,
        video: &mut FilterProcVideo<()>,
        params: &[(String, String)],
        text: &str,
    ) -> bool {
        if [
            "文字毎に個別オブジェクト",
            "自動スクロール",
            "移動座標上に表示",
        ]
        .iter()
        .any(|item| parameter_checked(params, item))
            || parameter_number(params, "表示速度").unwrap_or_default() != 0.0
            || parameter_string(params, "文字装飾").is_some_and(|v| v != "標準文字")
        {
            return false;
        }
        let (Some(font), Some(size), Some(align)) = (
            parameter_string(params, "フォント"),
            parameter_number(params, "サイズ"),
            parameter_string(params, "文字揃え").and_then(|v| text_alignment(&v)),
        ) else {
            return false;
        };
        let request = render::RenderRequest {
            text: text.into(),
            profile: None,
            font,
            size,
            align,
            spacing: parameter_number(params, "字間").unwrap_or_default(),
            line_spacing: parameter_number(params, "行間").unwrap_or_default(),
            color: parameter_string(params, "文字色").unwrap_or_else(|| "ffffff".into()),
            bold: parameter_checked(params, "B"),
            italic: parameter_checked(params, "I"),
            disable_ligatures: false,
        };
        let image =
            match crate::emoji_bypass::render(video, params, text, &request, &self.emoji_cache) {
                Ok(Some(image)) => image,
                Ok(None) => return false,
                Err(error) => {
                    crate::emoji_bypass::trace(&format!("bypass fallback: {error}"));
                    log_once(24, &format!("SVG emoji bypass skipped: {error}"));
                    return false;
                }
            };
        if video
            .set_image_resource_data(
                &ImageResource::Object,
                &image.bitmap.bgra,
                image.bitmap.width as u32,
                image.bitmap.height as u32,
                image.bitmap.width as u32 * 4,
                InputImageResourcePixelFormat::Bgra,
            )
            .is_err()
        {
            return false;
        }
        video.param.cx += image.cx;
        video.param.cy += image.cy;
        log_once(25, "ordered SVG emoji slots composited with native text");
        crate::emoji_bypass::trace("bypass uploaded successfully");
        true
    }

    fn render_ligatures(
        &self,
        video: &mut FilterProcVideo<()>,
        params: &[(String, String)],
        source: &str,
        profile_name: &str,
        antialias: shaping::AntialiasMethod,
    ) -> bool {
        // The opt-in path only handles the same static standard-text subset as
        // the v0.3 renderer. Any unsupported setting leaves the native path.
        if [
            "文字毎に個別オブジェクト",
            "自動スクロール",
            "移動座標上に表示",
        ]
        .iter()
        .any(|item| parameter_checked(params, item))
            || parameter_number(params, "表示速度").unwrap_or_default() != 0.0
            || parameter_string(params, "文字装飾").is_some_and(|value| value != "標準文字")
        {
            return false;
        }
        let Some(font) = parameter_string(params, "フォント") else {
            return false;
        };
        let Some(size) = parameter_number(params, "サイズ") else {
            return false;
        };
        let Some(align) =
            parameter_string(params, "文字揃え").and_then(|value| text_alignment(&value))
        else {
            return false;
        };
        let profile = {
            let Ok(mut profiles) = self.profiles.lock() else {
                return false;
            };
            profiles.refresh_if_changed();
            let Ok(profile) = profiles.store.profile(Some(profile_name)) else {
                return false;
            };
            profile.clone()
        };
        let request = render::RenderRequest {
            text: source.to_owned(),
            profile: Some(profile),
            font,
            size,
            spacing: parameter_number(params, "字間").unwrap_or_default(),
            line_spacing: parameter_number(params, "行間").unwrap_or_default(),
            color: parameter_string(params, "文字色").unwrap_or_else(|| "ffffff".to_owned()),
            align,
            bold: parameter_checked(params, "B"),
            italic: parameter_checked(params, "I"),
            disable_ligatures: false,
        };
        let cached = {
            let Ok(mut cache) = self.ligature_cache.lock() else {
                return false;
            };
            if let Some(index) = cache
                .iter()
                .position(|(key, mode, _)| key == &request && *mode == antialias)
            {
                let entry = cache.remove(index).expect("cached index exists");
                let bitmap = Arc::clone(&entry.2);
                cache.push_back(entry);
                Some(bitmap)
            } else {
                None
            }
        };
        let bitmap = if let Some(bitmap) = cached {
            bitmap
        } else {
            let section = video.read_section();
            let image = match render::render_ligature_opt_in(section, &request, antialias) {
                Ok(Some(image)) => image,
                Ok(None) => return false,
                Err(error) => {
                    log_once(19, &format!("ligature renderer failed: {error}"));
                    return false;
                }
            };
            let bitmap = Arc::new(image);
            if let Ok(mut cache) = self.ligature_cache.lock() {
                cache.push_back((request, antialias, Arc::clone(&bitmap)));
                while cache.len() > 16
                    || cache
                        .iter()
                        .map(|(_, _, image)| image.bgra.len())
                        .sum::<usize>()
                        > 64 * 1024 * 1024
                {
                    cache.pop_front();
                }
            }
            bitmap
        };
        let Ok(width) = u32::try_from(bitmap.width) else {
            return false;
        };
        let Ok(height) = u32::try_from(bitmap.height) else {
            return false;
        };
        let Some(pitch) = width.checked_mul(4) else {
            return false;
        };
        if video
            .set_image_resource_data(
                &ImageResource::Object,
                &bitmap.bgra,
                width,
                height,
                pitch,
                InputImageResourcePixelFormat::Bgra,
            )
            .is_err()
        {
            log_once(20, "could not upload opt-in ligature image");
            return false;
        }
        video.param.cx = (align % 3 - 1) as f32 * width as f32 / 2.0;
        video.param.cy = (align / 3 - 1) as f32 * height as f32 / 2.0;
        true
    }
}

fn text_alignment(value: &str) -> Option<i32> {
    Some(match value {
        "左寄せ[上]" => 0,
        "中央揃え[上]" => 1,
        "右寄せ[上]" => 2,
        "左寄せ[中]" => 3,
        "中央揃え[中]" => 4,
        "右寄せ[中]" => 5,
        "左寄せ[下]" => 6,
        "中央揃え[下]" => 7,
        "右寄せ[下]" => 8,
        _ => return None,
    })
}

fn parameter_string(params: &[(String, String)], name: &str) -> Option<String> {
    params
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| render::decode_alias(value))
}

fn parameter_checked(params: &[(String, String)], name: &str) -> bool {
    parameter_string(params, name).is_some_and(|value| is_checked(&value))
}

fn is_checked(value: &str) -> bool {
    matches!(value, "1" | "true" | "True")
}

fn parse_antialias(value: &str) -> shaping::AntialiasMethod {
    let value = value.trim();
    match value {
        "滑らか" => shaping::AntialiasMethod::NaturalSymmetric,
        "なし" => shaping::AntialiasMethod::Aliased,
        _ => match value.parse::<f64>() {
            Ok(1.0) => shaping::AntialiasMethod::NaturalSymmetric,
            Ok(2.0) => shaping::AntialiasMethod::Aliased,
            _ => shaping::AntialiasMethod::Natural,
        },
    }
}

fn native_text_parameters(object_alias: &str) -> Option<Vec<(String, String)>> {
    let table: aviutl2_alias::Table = object_alias.parse().ok()?;
    let object = table.get_table("Object").or_else(|| table.get_table("0"))?;
    let mut text_effects = object.subtables().filter_map(|(_, effect)| {
        (effect.get_value("effect.name").map(String::as_str) == Some("テキスト")).then_some(effect)
    });
    let text_effect = text_effects.next()?;
    if text_effects.next().is_some() {
        return None;
    }
    Some(
        text_effect
            .values()
            .filter(|(key, _)| key.as_str() != "effect.name")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn parameter_number(params: &[(String, String)], name: &str) -> Option<f64> {
    let value = params.iter().find(|(key, _)| key == name)?.1.as_str();
    let value = render::decode_alias(value);
    let value = value.parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

pub(crate) fn encode_alias(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => encoded.push_str("\\\\"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character => encoded.push(character),
        }
    }
    encoded
}

#[cfg(test)]
mod antialias_tests {
    use super::*;

    #[test]
    fn selection_accepts_numeric_alias_formats_and_labels() {
        assert_eq!(parse_antialias("0"), shaping::AntialiasMethod::Natural);
        assert_eq!(
            parse_antialias("1.00"),
            shaping::AntialiasMethod::NaturalSymmetric
        );
        assert_eq!(
            parse_antialias("滑らか"),
            shaping::AntialiasMethod::NaturalSymmetric
        );
        assert_eq!(parse_antialias("2.0"), shaping::AntialiasMethod::Aliased);
        assert_eq!(parse_antialias("なし"), shaping::AntialiasMethod::Aliased);
    }

    #[test]
    fn host_font_calls_run_without_profile_lock_and_results_are_cached() {
        struct Coverage<'a> {
            profiles: &'a Mutex<ReloadableProfiles>,
            calls: usize,
        }
        impl super::super::GlyphCoverage for Coverage<'_> {
            fn has_glyph(&mut self, _font: &str, _character: char) -> Option<bool> {
                assert!(
                    self.profiles.try_lock().is_ok(),
                    "host call held the profile lock"
                );
                self.calls += 1;
                Some(true)
            }
        }
        let mut profile = compositefont_core::CompositeFontProfile::neutral_default();
        profile.western.font_family = "First".into();
        profile
            .western_fallbacks
            .push(compositefont_core::FontAdjustment {
                font_family: "Last".into(),
                ..compositefont_core::FontAdjustment::neutral()
            });
        let mut profiles = ReloadableProfiles::new(std::env::temp_dir().join(format!(
            "comfont-native-lock-{}-missing.json",
            std::process::id()
        )));
        profiles.store = super::super::ProfileStore::from_profiles(vec![profile]);
        let profiles = Mutex::new(profiles);
        let mut coverage = Coverage {
            profiles: &profiles,
            calls: 0,
        };
        for _ in 0..2 {
            assert_eq!(
                decorate_native_text(
                    &profiles,
                    "A",
                    "default",
                    Some(40.0),
                    Some(0.0),
                    &mut coverage
                ),
                "<@First>A<@>"
            );
        }
        assert_eq!(coverage.calls, 1);
    }
}
