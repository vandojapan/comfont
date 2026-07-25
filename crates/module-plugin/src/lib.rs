use aviutl2::module::{IntoScriptModuleReturnValue, ScriptModule, ScriptModuleFunctions};
#[cfg(debug_assertions)]
use compositefont_core::FontAdjustment;
use compositefont_core::{
    CompositeFontProfile, DEFAULT_PROFILE_NAME, PROFILE_SCHEMA_VERSION, ProfileDocument,
    ProfileStore, ResolvedFont, codepoint_to_char, single_codepoint,
};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant, SystemTime},
};

const API_VERSION: i32 = 3;
#[cfg(debug_assertions)]
const INTEGRATION_TEST_PROFILE: &str = "integration-test";

#[cfg(debug_assertions)]
fn integration_test_profile() -> CompositeFontProfile {
    let western = FontAdjustment {
        font_family: "Arial Black".to_owned(),
        size_ratio: 1.0,
        baseline_shift_em: 0.0,
        tracking_adjust_em: 0.05,
        vertical_scale_ratio: 1.1,
        horizontal_scale_ratio: 0.9,
    };
    let japanese = FontAdjustment {
        font_family: "游明朝".to_owned(),
        size_ratio: 1.0,
        baseline_shift_em: 0.0,
        tracking_adjust_em: 0.1,
        vertical_scale_ratio: 0.9,
        horizontal_scale_ratio: 1.1,
    };

    CompositeFontProfile {
        name: INTEGRATION_TEST_PROFILE.to_owned(),
        western: western.clone(),
        hiragana: japanese.clone(),
        katakana: japanese.clone(),
        kanji: japanese.clone(),
        digit: western,
        symbol: japanese.clone(),
        other: japanese,
    }
}

#[cfg(debug_assertions)]
fn builtin_profiles() -> Vec<CompositeFontProfile> {
    vec![
        CompositeFontProfile::neutral_default(),
        integration_test_profile(),
    ]
}

#[cfg(not(debug_assertions))]
fn builtin_profiles() -> Vec<CompositeFontProfile> {
    vec![CompositeFontProfile::neutral_default()]
}

fn profile_path() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("compositefont")
        .join("profiles.json")
}

fn load_profile_store(path: &Path) -> Result<ProfileStore, String> {
    let mut profiles = match std::fs::read(path) {
        Ok(bytes) => {
            let document: ProfileDocument = serde_json::from_slice(&bytes)
                .map_err(|error| format!("{}の形式が不正です: {error}", path.display()))?;
            if document.schema_version != PROFILE_SCHEMA_VERSION {
                return Err(format!(
                    "{}のschema_version {}には対応していません",
                    path.display(),
                    document.schema_version
                ));
            }
            if document.profiles.is_empty() {
                return Err(format!("{}にプロファイルがありません", path.display()));
            }
            if !document
                .profiles
                .iter()
                .any(|profile| profile.name == DEFAULT_PROFILE_NAME)
            {
                return Err(format!(
                    "{}にdefaultプロファイルがありません",
                    path.display()
                ));
            }
            document.profiles
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => builtin_profiles(),
        Err(error) => return Err(format!("{}を読み込めません: {error}", path.display())),
    };

    #[cfg(debug_assertions)]
    {
        profiles.retain(|profile| profile.name != INTEGRATION_TEST_PROFILE);
        profiles.push(integration_test_profile());
    }
    Ok(ProfileStore::from_profiles(profiles))
}

struct ReloadableProfiles {
    store: ProfileStore,
    path: PathBuf,
    observed_modified: Option<SystemTime>,
    last_checked: Instant,
}

impl ReloadableProfiles {
    fn new(path: PathBuf) -> Self {
        let observed_modified = modified_time(&path);
        let store = load_profile_store(&path).unwrap_or_else(|error| {
            aviutl2::logger::write_error_log(&error).ok();
            ProfileStore::from_profiles(builtin_profiles())
        });
        Self {
            store,
            path,
            observed_modified,
            last_checked: Instant::now(),
        }
    }

    fn resolve(
        &mut self,
        codepoint: char,
        profile_name: Option<&str>,
    ) -> Result<ResolvedFont, compositefont_core::ResolveError> {
        self.refresh_if_changed();
        self.store.resolve(codepoint, profile_name)
    }

    fn refresh_if_changed(&mut self) {
        if self.last_checked.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_checked = Instant::now();
        let modified = modified_time(&self.path);
        if modified == self.observed_modified {
            return;
        }
        self.observed_modified = modified;
        match load_profile_store(&self.path) {
            Ok(store) => self.store = store,
            Err(error) => {
                aviutl2::logger::write_error_log(&error).ok();
            }
        }
    }
}

fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// Luaへ公開する多値返却。aviutl2依存をcoreへ持ち込まないための境界型。
struct ScriptResolvedFont(ResolvedFont);

impl From<ResolvedFont> for ScriptResolvedFont {
    fn from(value: ResolvedFont) -> Self {
        Self(value)
    }
}

impl IntoScriptModuleReturnValue for ScriptResolvedFont {
    type Err = anyhow::Error;

    fn into_return_values(
        self,
    ) -> Result<Vec<aviutl2::module::ScriptModuleReturnValue>, Self::Err> {
        let value = self.0;
        (
            value.font_family,
            value.size_ratio,
            value.baseline_shift_em,
            value.tracking_adjust_em,
            value.vertical_scale_ratio,
            value.horizontal_scale_ratio,
            value.category,
            value.rule_id,
        )
            .into_return_values()
    }
}

/// Luaから`compositefont`として参照されるスクリプトモジュール。
#[aviutl2::plugin(ScriptModule)]
struct CompositeFontModule {
    profiles: Mutex<ReloadableProfiles>,
}

impl ScriptModule for CompositeFontModule {
    fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
        Ok(Self {
            profiles: Mutex::new(ReloadableProfiles::new(profile_path())),
        })
    }

    fn plugin_info(&self) -> aviutl2::module::ScriptModuleTable {
        aviutl2::module::ScriptModuleTable {
            information: format!(
                "Composite Font script module for AviUtl2 / v{} / API v{API_VERSION}",
                env!("CARGO_PKG_VERSION")
            ),
            functions: Self::functions(),
        }
    }
}

#[aviutl2::module::functions]
impl CompositeFontModule {
    fn api_version() -> i32 {
        API_VERSION
    }

    fn resolve(
        &self,
        character: String,
        profile: Option<String>,
    ) -> aviutl2::AnyResult<ScriptResolvedFont> {
        let codepoint = single_codepoint(&character)?;
        let mut profiles = self
            .profiles
            .lock()
            .map_err(|_| anyhow::anyhow!("composite font profile lock was poisoned"))?;
        Ok(profiles.resolve(codepoint, profile.as_deref())?.into())
    }

    fn resolve_codepoint(
        &self,
        codepoint: u32,
        profile: Option<String>,
    ) -> aviutl2::AnyResult<ScriptResolvedFont> {
        let codepoint = codepoint_to_char(codepoint)?;
        let mut profiles = self
            .profiles
            .lock()
            .map_err(|_| anyhow::anyhow!("composite font profile lock was poisoned"))?;
        Ok(profiles.resolve(codepoint, profile.as_deref())?.into())
    }
}

aviutl2::register_script_module!(CompositeFontModule);

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temporary_profile_path() -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "compositefont-module-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn result_is_exported_as_eight_lua_values() {
        let resolved = ResolvedFont {
            font_family: "Inter".to_owned(),
            size_ratio: 0.95,
            baseline_shift_em: 0.02,
            tracking_adjust_em: -0.01,
            vertical_scale_ratio: 0.9,
            horizontal_scale_ratio: 1.1,
            category: "latin".to_owned(),
            rule_id: "latin-uppercase".to_owned(),
        };
        let values = ScriptResolvedFont(resolved).into_return_values().unwrap();

        assert_eq!(values.len(), 8);
        assert!(matches!(
            &values[0],
            aviutl2::module::ScriptModuleReturnValue::String(value) if value == "Inter"
        ));
        assert!(matches!(
            values[1],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 0.95
        ));
        assert!(matches!(
            values[4],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 0.9
        ));
        assert!(matches!(
            values[5],
            aviutl2::module::ScriptModuleReturnValue::Float(value) if value == 1.1
        ));
    }

    #[test]
    fn integration_profile_resolves_all_categories() {
        let profile = integration_test_profile();

        assert_eq!(profile.resolve('A').font_family, "Arial Black");
        assert_eq!(profile.resolve('A').tracking_adjust_em, 0.05);
        assert_eq!(profile.resolve('A').vertical_scale_ratio, 1.1);
        assert_eq!(profile.resolve('A').horizontal_scale_ratio, 0.9);
        assert_eq!(profile.resolve('あ').font_family, "游明朝");
        assert_eq!(profile.resolve('あ').tracking_adjust_em, 0.1);
        assert_eq!(profile.resolve('あ').vertical_scale_ratio, 0.9);
        assert_eq!(profile.resolve('あ').horizontal_scale_ratio, 1.1);
        assert_eq!(profile.resolve('カ').font_family, "游明朝");
        assert_eq!(profile.resolve('日').font_family, "游明朝");
        assert_eq!(profile.resolve('1').font_family, "Arial Black");
        assert_eq!(profile.resolve('。').font_family, "游明朝");
        assert_eq!(profile.resolve('Ж').font_family, "游明朝");
    }

    #[test]
    fn loads_profiles_saved_by_the_editor() {
        let path = temporary_profile_path();
        let mut document = ProfileDocument::with_builtin_default();
        document.profiles[0].kanji.font_family = "Arial Black".to_owned();
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();

        let store = load_profile_store(&path).unwrap();
        assert_eq!(
            store.resolve('漢', Some("default")).unwrap().font_family,
            "Arial Black"
        );

        std::fs::remove_file(path).unwrap();
    }
}
