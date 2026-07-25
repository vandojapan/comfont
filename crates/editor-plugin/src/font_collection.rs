use std::{
    collections::BTreeSet,
    ffi::OsStr,
    ffi::c_void,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use windows::{
    Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory, IDWriteFactory5,
        IDWriteFontCollection,
    },
    core::{Interface, PCWSTR},
};

pub struct RegisteredFontCollection {
    collection: IDWriteFontCollection,
    font_file_count: usize,
}

pub type RegisterFontCollectionFn = unsafe extern "C" fn(*mut c_void);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontRegistrationOutcome {
    Added { font_file_count: usize },
    Unchanged,
    NoFonts,
}

#[derive(Default)]
pub struct FontRegistrationState {
    collections: Vec<RegisteredFontCollection>,
    registered_paths: BTreeSet<PathBuf>,
}

impl FontRegistrationState {
    pub fn register_initial_fonts(
        &mut self,
        registrar: RegisterFontCollectionFn,
    ) -> Result<FontRegistrationOutcome, String> {
        let directory = ensure_font_directory()?;
        let font_files = collect_font_files(&directory)?;
        if font_files.is_empty() {
            return Ok(FontRegistrationOutcome::NoFonts);
        }
        let new_font_files = unregistered_font_files(font_files, &self.registered_paths);
        if new_font_files.is_empty() {
            return Ok(FontRegistrationOutcome::Unchanged);
        }

        let registration = load_from_files(directory, &new_font_files)?;
        let raw_collection = registration.collection().as_raw();
        let font_file_count = registration.font_file_count();
        unsafe {
            registrar(raw_collection);
        }
        self.collections.push(registration);
        self.registered_paths.extend(new_font_files);
        Ok(FontRegistrationOutcome::Added { font_file_count })
    }

    pub fn pending_font_file_count(&self) -> Result<usize, String> {
        let directory = ensure_font_directory()?;
        let font_files = collect_font_files(&directory)?;
        Ok(unregistered_font_files(font_files, &self.registered_paths).len())
    }

    pub fn registered_collection_count(&self) -> usize {
        self.collections.len()
    }
}

impl RegisteredFontCollection {
    pub fn collection(&self) -> &IDWriteFontCollection {
        &self.collection
    }

    pub fn font_file_count(&self) -> usize {
        self.font_file_count
    }
}

pub fn font_directory() -> PathBuf {
    aviutl2::config::app_data_path()
        .join("compositefont")
        .join("fonts")
}

fn ensure_font_directory() -> Result<PathBuf, String> {
    let directory = font_directory();
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    Ok(directory)
}

fn load_from_files(
    directory: PathBuf,
    font_files: &[PathBuf],
) -> Result<RegisteredFontCollection, String> {
    let factory: IDWriteFactory5 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
        .map_err(|error| format!("DWriteCreateFactory failed: {error}"))?;
    let base_factory: IDWriteFactory = factory
        .cast()
        .map_err(|error| format!("IDWriteFactory query failed: {error}"))?;
    let builder = unsafe { factory.CreateFontSetBuilder() }
        .map_err(|error| format!("CreateFontSetBuilder failed: {error}"))?;

    let mut loaded_count = 0;
    for path in font_files {
        let canonical_path = match path.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                log_skipped_font(path, &error.to_string());
                continue;
            }
        };
        let wide_path = wide_null(canonical_path.as_os_str());
        let font_file =
            match unsafe { base_factory.CreateFontFileReference(PCWSTR(wide_path.as_ptr()), None) }
            {
                Ok(file) => file,
                Err(error) => {
                    log_skipped_font(path, &error.to_string());
                    continue;
                }
            };
        if let Err(error) = unsafe { builder.AddFontFile(&font_file) } {
            log_skipped_font(path, &error.to_string());
            continue;
        }
        loaded_count += 1;
    }

    if loaded_count == 0 {
        return Err(format!(
            "none of the font files in {} could be loaded",
            directory.display()
        ));
    }

    let font_set = unsafe { builder.CreateFontSet() }
        .map_err(|error| format!("CreateFontSet failed: {error}"))?;
    let collection = unsafe { factory.CreateFontCollectionFromFontSet(&font_set) }
        .map_err(|error| format!("CreateFontCollectionFromFontSet failed: {error}"))?;
    let collection = collection
        .cast::<IDWriteFontCollection>()
        .map_err(|error| format!("IDWriteFontCollection query failed: {error}"))?;

    Ok(RegisteredFontCollection {
        collection,
        font_file_count: loaded_count,
    })
}

fn log_skipped_font(path: &Path, reason: &str) {
    let _ = aviutl2::logger::write_warn_log(&format!(
        "Composite Font: skipped private font {}: {reason}",
        path.display()
    ));
}

fn collect_font_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    collect_font_files_recursive(directory, &mut result)?;
    result.sort_unstable();
    Ok(result)
}

fn unregistered_font_files(
    font_files: Vec<PathBuf>,
    registered_paths: &BTreeSet<PathBuf>,
) -> Vec<PathBuf> {
    font_files
        .into_iter()
        .filter(|path| !registered_paths.contains(path))
        .collect()
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

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temporary_directory() -> PathBuf {
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        std::env::temp_dir().join(format!(
            "compositefont-font-loader-{}-{}",
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
    fn registered_paths_leave_only_new_font_files() {
        let directory = temporary_directory();
        std::fs::create_dir_all(&directory).unwrap();
        let old_font = directory.join("old.ttf");
        let new_font = directory.join("new.otf");
        std::fs::write(&old_font, []).unwrap();
        std::fs::write(&new_font, []).unwrap();

        let files = collect_font_files(&directory).unwrap();
        let registered_paths = BTreeSet::from([old_font]);
        let additions = unregistered_font_files(files, &registered_paths);
        assert_eq!(additions, vec![new_font]);

        std::fs::remove_dir_all(directory).unwrap();
    }
}
