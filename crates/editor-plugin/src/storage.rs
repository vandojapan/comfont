use std::path::{Path, PathBuf};

use compositefont_core::{DEFAULT_PROFILE_NAME, PROFILE_SCHEMA_VERSION, ProfileDocument};

pub fn default_profile_path() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("compositefont")
        .join("profiles.json")
}

pub fn load_document(path: &Path) -> Result<ProfileDocument, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ProfileDocument::with_builtin_default());
        }
        Err(error) => return Err(format!("{}を読み込めません: {error}", path.display())),
    };
    let document: ProfileDocument = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{}の形式が不正です: {error}", path.display()))?;
    validate_document(&document)?;
    Ok(document)
}

pub fn save_document(path: &Path, document: &ProfileDocument) -> Result<(), String> {
    validate_document(document)?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("保存先{}に親フォルダがありません", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("{}を作成できません: {error}", parent.display()))?;
    let json = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("プロファイルをJSONへ変換できません: {error}"))?;
    std::fs::write(path, json)
        .map_err(|error| format!("{}へ保存できません: {error}", path.display()))
}

fn validate_document(document: &ProfileDocument) -> Result<(), String> {
    if document.schema_version != PROFILE_SCHEMA_VERSION {
        return Err(format!(
            "未対応のプロファイル形式です（schema_version: {}、対応: {}）",
            document.schema_version, PROFILE_SCHEMA_VERSION
        ));
    }
    if document.profiles.is_empty() {
        return Err("プロファイルが1件もありません。".to_owned());
    }
    if !document
        .profiles
        .iter()
        .any(|profile| profile.name == DEFAULT_PROFILE_NAME)
    {
        return Err("defaultプロファイルがありません。".to_owned());
    }
    for (index, profile) in document.profiles.iter().enumerate() {
        if profile.name.trim().is_empty() {
            return Err(format!("{}件目のプロファイル名が空です。", index + 1));
        }
        if document.profiles[..index]
            .iter()
            .any(|other| other.name == profile.name)
        {
            return Err(format!(
                "プロファイル名「{}」が重複しています。",
                profile.name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temporary_path(name: &str) -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "compositefont-{name}-{}-{}.json",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn missing_file_returns_builtin_default() {
        let path = temporary_path("missing");
        let document = load_document(&path).unwrap();
        assert_eq!(document.profiles[0].name, "default");
    }

    #[test]
    fn saves_and_loads_profiles() {
        let path = temporary_path("roundtrip");
        let mut document = ProfileDocument::with_builtin_default();
        document.profiles[0].western.font_family = "DIN 2014".to_owned();

        save_document(&path, &document).unwrap();
        let loaded = load_document(&path).unwrap();
        assert_eq!(loaded, document);

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn loads_profiles_created_before_scale_fields_existed() {
        let path = temporary_path("legacy-scale-defaults");
        let document = ProfileDocument::with_builtin_default();
        let mut json = serde_json::to_value(document).unwrap();
        let profile = json["profiles"][0].as_object_mut().unwrap();
        for category in [
            "western", "hiragana", "katakana", "kanji", "digit", "symbol", "other",
        ] {
            let adjustment = profile[category].as_object_mut().unwrap();
            adjustment.remove("vertical_scale_ratio");
            adjustment.remove("horizontal_scale_ratio");
        }
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        let loaded = load_document(&path).unwrap();
        assert_eq!(loaded.profiles[0].western.vertical_scale_ratio, 1.0);
        assert_eq!(loaded.profiles[0].western.horizontal_scale_ratio, 1.0);

        std::fs::remove_file(path).unwrap();
    }
}
