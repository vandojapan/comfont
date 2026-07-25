use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontStageOutcome {
    pub source_file_count: usize,
    pub added_file_count: usize,
    pub existing_file_count: usize,
}

#[derive(Default)]
pub struct FontRegistrationState;

impl FontRegistrationState {
    pub fn stage_fonts_for_next_launch(&mut self) -> Result<FontStageOutcome, String> {
        let source = ensure_font_directory()?;
        let destination = host_font_directory();
        stage_font_files(&source, &destination)
    }
}

/// Composite Fontが追加フォントの投入元として監視するフォルダー。
pub fn font_directory() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("compositefont")
        .join("fonts")
}

/// AviUtl2が標準UIの構築前に読み込む公式のフォントフォルダー。
pub fn host_font_directory() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("Font")
        .join("compositefont")
}

fn ensure_font_directory() -> Result<PathBuf, String> {
    let directory = font_directory();
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    Ok(directory)
}

fn stage_font_files(source: &Path, destination: &Path) -> Result<FontStageOutcome, String> {
    let font_files = collect_font_files(source)?;
    if font_files.is_empty() {
        return Ok(FontStageOutcome::default());
    }

    let mut files_by_name = BTreeMap::<OsString, PathBuf>::new();
    for path in font_files {
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("font path has no file name: {}", path.display()))?
            .to_owned();
        if let Some(previous) = files_by_name.insert(file_name.clone(), path.clone()) {
            return Err(format!(
                "duplicate private font file name {}: {} and {}",
                file_name.to_string_lossy(),
                previous.display(),
                path.display()
            ));
        }
    }

    std::fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;

    let mut outcome = FontStageOutcome {
        source_file_count: files_by_name.len(),
        ..Default::default()
    };
    for (file_name, source_path) in files_by_name {
        let destination_path = destination.join(file_name);
        if destination_path.exists() {
            if !destination_path.is_file() {
                return Err(format!(
                    "font destination is not a file: {}",
                    destination_path.display()
                ));
            }
            outcome.existing_file_count += 1;
            continue;
        }
        std::fs::copy(&source_path, &destination_path).map_err(|error| {
            format!(
                "cannot copy {} to {}: {error}",
                source_path.display(),
                destination_path.display()
            )
        })?;
        outcome.added_file_count += 1;
    }
    Ok(outcome)
}

fn collect_font_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    collect_font_files_recursive(directory, &mut result)?;
    result.sort_unstable();
    Ok(result)
}

fn collect_font_files_recursive(directory: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read directory entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_font_files_recursive(&entry.path(), result)?;
        } else if file_type.is_file() && is_supported_font_path(&entry.path()) {
            result.push(entry.path());
        }
    }
    Ok(())
}

fn is_supported_font_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "ttf" | "otf" | "ttc" | "otc"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temporary_directory() -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "compositefont-font-staging-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn recognizes_supported_font_extensions_case_insensitively() {
        for name in ["a.ttf", "b.OTF", "c.Ttc", "d.oTc"] {
            assert!(is_supported_font_path(Path::new(name)), "{name}");
        }
        for name in ["a.woff2", "b.txt", "no-extension"] {
            assert!(!is_supported_font_path(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn collects_supported_fonts_recursively_in_stable_order() {
        let directory = temporary_directory();
        let nested = directory.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(directory.join("z.otf"), []).unwrap();
        std::fs::write(directory.join("ignored.txt"), []).unwrap();
        std::fs::write(nested.join("a.TTF"), []).unwrap();

        let files = collect_font_files(&directory).unwrap();
        assert_eq!(
            files,
            vec![directory.join("nested/a.TTF"), directory.join("z.otf")]
        );

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn stages_new_fonts_without_overwriting_existing_files() {
        let root = temporary_directory();
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(source.join("nested")).unwrap();
        std::fs::write(source.join("nested/test.ttf"), [1, 2, 3]).unwrap();

        let first = stage_font_files(&source, &destination).unwrap();
        assert_eq!(first.source_file_count, 1);
        assert_eq!(first.added_file_count, 1);
        assert_eq!(first.existing_file_count, 0);
        assert_eq!(
            std::fs::read(destination.join("test.ttf")).unwrap(),
            [1, 2, 3]
        );

        std::fs::write(source.join("nested/test.ttf"), [9, 9, 9]).unwrap();
        let second = stage_font_files(&source, &destination).unwrap();
        assert_eq!(second.added_file_count, 0);
        assert_eq!(second.existing_file_count, 1);
        assert_eq!(
            std::fs::read(destination.join("test.ttf")).unwrap(),
            [1, 2, 3]
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_duplicate_file_names_from_nested_source_directories() {
        let root = temporary_directory();
        let source = root.join("source");
        let destination = root.join("destination");
        std::fs::create_dir_all(source.join("a")).unwrap();
        std::fs::create_dir_all(source.join("b")).unwrap();
        std::fs::write(source.join("a/font.otf"), []).unwrap();
        std::fs::write(source.join("b/font.otf"), []).unwrap();

        let error = stage_font_files(&source, &destination).unwrap_err();
        assert!(error.contains("duplicate private font file name"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
