use compositefont_core::{ProfileDocument, profile_selection_id, profile_selection_label};
use std::{collections::BTreeMap, path::Path};
const TEMPLATE: &str = include_str!("../../../scripts/合成フォント.anm2");

pub fn generate(document: &ProfileDocument) -> Result<String, String> {
    let mut ids = BTreeMap::new();
    let mut labels = std::collections::BTreeSet::from([String::from("無装飾")]);
    let mut choices = String::from("--select@selection:comfont::プロファイル選択=0,無装飾=-1");
    for profile in &document.profiles {
        let id = profile_selection_id(&profile.name);
        if let Some(previous) = ids.insert(id, &profile.name) {
            return Err(format!(
                "プロファイル選択IDが競合しています: {previous} / {}",
                profile.name
            ));
        }
        let label = profile_selection_label(&profile.name);
        if !labels.insert(label.clone()) {
            return Err(format!("anm2の選択表示名が競合しています: {label}"));
        }
        choices.push_str(&format!(",{label}={id}"));
    }
    Ok(TEMPLATE
        .lines()
        .map(|line| {
            if line.starts_with("--select@selection:") {
                choices.as_str()
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n")
}

pub fn update(path: &Path, document: &ProfileDocument) -> Result<(), String> {
    let script = generate(document)?;
    if std::fs::read(path).ok().as_deref() == Some(script.as_bytes()) {
        return Ok(());
    }
    let parent = path.parent().ok_or("anm2の保存先が不正です")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    std::fs::write(path, script).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_survives_reordering_and_does_not_reuse_deleted_profiles() {
        let mut doc = ProfileDocument::with_builtin_default();
        let mut p = doc.profiles[0].clone();
        p.name = "字幕".to_owned();
        doc.profiles.push(p);
        let id = profile_selection_id("字幕");
        assert!(generate(&doc).unwrap().contains(&format!(",字幕={id}")));
        doc.profiles.reverse();
        assert!(generate(&doc).unwrap().contains(&format!(",字幕={id}")));
        assert_ne!(id, profile_selection_id("別の字幕"));
        assert_eq!(profile_selection_id("default"), 0);
    }
    #[test]
    fn labels_cannot_inject_directives() {
        let mut doc = ProfileDocument::with_builtin_default();
        let mut p = doc.profiles[0].clone();
        p.name = "a,b=1\n--track@x:evil".to_owned();
        doc.profiles.push(p);
        let script = generate(&doc).unwrap();
        assert!(!script.lines().any(|line| line.starts_with("--track@x")));
        assert!(script.contains("a，b＝1 --track@x：evil="));
    }
    #[test]
    fn ambiguous_saved_labels_are_rejected() {
        for names in [vec!["無装飾"], vec!["a,b", "a，b"]] {
            let mut doc = ProfileDocument::with_builtin_default();
            for name in names {
                let mut p = doc.profiles[0].clone();
                p.name = name.into();
                doc.profiles.push(p);
            }
            assert!(generate(&doc).is_err());
        }
    }
    #[test]
    fn duplicate_ids_are_rejected() {
        let mut doc = ProfileDocument::with_builtin_default();
        doc.profiles.push(doc.profiles[0].clone());
        assert!(generate(&doc).is_err());
    }
}
