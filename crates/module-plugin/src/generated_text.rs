//! Frame-local text produced by Lua; no script is evaluated or written back here.
use std::{
    collections::VecDeque,
    sync::{Mutex, OnceLock},
};

/// The Lua module and native filter are separate subplugins in the same DLL.
/// Keep their bounded hand-off cache shared, without holding its lock in host calls.
pub(crate) fn shared_cache() -> &'static Mutex<GeneratedTextCache> {
    static CACHE: OnceLock<Mutex<GeneratedTextCache>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

const TEXT_LIMIT: usize = 64 * 1024;
const ENTRY_LIMIT: usize = 64;
const BYTE_LIMIT: usize = 4 * 1024 * 1024;

struct Entry {
    object_id: String,
    frame: f64,
    source: String,
    text: String,
    producer: Option<(String, String)>,
}

#[derive(Default)]
pub(crate) struct GeneratedTextCache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl GeneratedTextCache {
    pub(crate) fn forget(&mut self, object_id: &str, frame: f64) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.object_id == object_id && e.frame == frame)
        {
            self.remove(index);
        }
    }

    #[cfg(test)]
    pub(crate) fn capture(
        &mut self,
        text: String,
        object_id: String,
        frame: f64,
        source: String,
    ) -> bool {
        self.capture_from(text, object_id, frame, source, None)
    }

    pub(crate) fn capture_from(
        &mut self,
        text: String,
        object_id: String,
        frame: f64,
        source: String,
        producer: Option<(String, String)>,
    ) -> bool {
        // Remove the previous submission for this frame even if the new one is
        // invalid: a failed submission must never leave an old result visible.
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.object_id == object_id && e.frame == frame)
        {
            self.remove(index);
        }
        if object_id.is_empty()
            || object_id.len() > 128
            || !frame.is_finite()
            || source.len() > TEXT_LIMIT
            || text.len() > TEXT_LIMIT
            || source.contains('\0')
            || text.contains('\0')
            || object_id.contains('\0')
            || producer.as_ref().is_some_and(|(effect, script)| {
                effect.is_empty()
                    || effect.len() > 128
                    || effect.contains('\0')
                    || script.len() > TEXT_LIMIT
                    || script.contains('\0')
            })
        {
            return false;
        }
        self.bytes += object_id.len()
            + source.len()
            + text.len()
            + producer
                .as_ref()
                .map_or(0, |(effect, script)| effect.len() + script.len());
        self.entries.push_back(Entry {
            object_id,
            frame,
            source,
            text,
            producer,
        });
        while self.entries.len() > ENTRY_LIMIT || self.bytes > BYTE_LIMIT {
            self.remove(0);
        }
        true
    }

    pub(crate) fn producer(
        &self,
        object_id: &str,
        frame: f64,
        source: &str,
    ) -> Option<(String, String)> {
        self.entries
            .iter()
            .find(|e| e.object_id == object_id && e.frame == frame && e.source == source)?
            .producer
            .clone()
    }

    pub(crate) fn get(&mut self, object_id: &str, frame: f64, source: &str) -> Option<String> {
        let index = self
            .entries
            .iter()
            .position(|e| e.object_id == object_id && e.frame == frame && e.source == source)?;
        let entry = self.entries.remove(index)?;
        let text = entry.text.clone();
        self.entries.push_back(entry);
        Some(text)
    }

    fn remove(&mut self, index: usize) {
        if let Some(entry) = self.entries.remove(index) {
            self.bytes -= entry.object_id.len() + entry.source.len() + entry.text.len();
            self.bytes -= entry
                .producer
                .as_ref()
                .map_or(0, |(effect, script)| effect.len() + script.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolates_objects_frames_and_exact_source_without_decoding_generated_text() {
        let mut cache = GeneratedTextCache::default();
        let source = r"<?obj.mes('x')?>\n";
        let generated = "କ୍ଷ\n日本語\\n<#red>ABC";
        assert!(cache.capture(generated.into(), "42".into(), 1.5, source.into()));
        assert_eq!(cache.get("42", 1.5, source).as_deref(), Some(generated));
        assert!(cache.get("43", 1.5, source).is_none());
        assert!(cache.get("42", 2.0, source).is_none());
        assert!(cache.get("42", 1.5, "<?obj.mes('y')?>").is_none());
        // A cached host text stage may be reused while the profile changes.
        assert_eq!(cache.get("42", 1.5, source).as_deref(), Some(generated));
    }

    #[test]
    fn updated_and_empty_submissions_replace_the_previous_frame_result() {
        let mut cache = GeneratedTextCache::default();
        for text in ["first", "changed", ""] {
            assert!(cache.capture(text.into(), "42".into(), 0.0, "source".into()));
            assert_eq!(cache.get("42", -0.0, "source").as_deref(), Some(text));
            assert_eq!(cache.entries.len(), 1);
        }
        assert!(cache.capture("new source".into(), "42".into(), 0.0, "updated".into()));
        assert!(cache.get("42", 0.0, "source").is_none());
    }

    #[test]
    fn forget_removes_only_the_requested_object_and_frame() {
        let mut cache = GeneratedTextCache::default();
        for (id, frame) in [("42", 0.0), ("42", 1.0), ("43", 0.0)] {
            assert!(cache.capture("text".into(), id.into(), frame, "source".into()));
        }
        cache.forget("42", 0.0);
        cache.forget("42", 0.0);
        assert!(cache.get("42", 0.0, "source").is_none());
        assert!(cache.get("42", 1.0, "source").is_some());
        assert!(cache.get("43", 0.0, "source").is_some());
        assert_eq!(cache.bytes, 2 * (2 + 4 + 6));
    }

    #[test]
    fn producer_metadata_is_isolated_bounded_and_replaced_with_text() {
        let mut cache = GeneratedTextCache::default();
        let producer = Some(("スクリプト制御:1".to_owned(), "generated script".to_owned()));
        assert!(cache.capture_from(
            "text".into(),
            "42".into(),
            0.0,
            "source".into(),
            producer.clone()
        ));
        assert_eq!(cache.producer("42", 0.0, "source"), producer);
        assert!(cache.producer("43", 0.0, "source").is_none());
        assert!(!cache.capture_from(
            "text".into(),
            "42".into(),
            0.0,
            "source".into(),
            Some(("effect".into(), "x".repeat(TEXT_LIMIT + 1)))
        ));
        assert!(cache.get("42", 0.0, "source").is_none());
        assert_eq!(cache.bytes, 0);
        assert!(cache.capture("inline".into(), "42".into(), 0.0, "source".into()));
        assert!(cache.producer("42", 0.0, "source").is_none());
    }

    #[test]
    fn invalid_submissions_remove_stale_results_and_reject_nonfinite_frames() {
        let mut cache = GeneratedTextCache::default();
        assert!(cache.capture("old".into(), "42".into(), 0.0, "source".into()));
        assert!(!cache.capture(
            "x".repeat(TEXT_LIMIT + 1),
            "42".into(),
            0.0,
            "source".into()
        ));
        assert!(cache.get("42", 0.0, "source").is_none());
        for frame in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(!cache.capture("text".into(), "42".into(), frame, "source".into()));
        }
        assert!(!cache.capture("text".into(), "".into(), 0.0, "source".into()));
        assert!(!cache.capture("text\0".into(), "42".into(), 0.0, "source".into()));
        assert!(!cache.capture("text".into(), "42".into(), 0.0, "x".repeat(TEXT_LIMIT + 1)));
    }

    #[test]
    fn entry_and_byte_limits_evict_oldest_without_evicting_recently_read_text() {
        let mut cache = GeneratedTextCache::default();
        for i in 0..ENTRY_LIMIT {
            assert!(cache.capture("text".into(), i.to_string(), 0.0, "source".into()));
        }
        assert!(cache.get("0", 0.0, "source").is_some());
        assert!(cache.capture("text".into(), "extra".into(), 0.0, "source".into()));
        assert!(cache.get("1", 0.0, "source").is_none());
        assert!(cache.get("0", 0.0, "source").is_some());
        let mut cache = GeneratedTextCache::default();
        for i in 0..ENTRY_LIMIT {
            assert!(cache.capture(
                "t".repeat(TEXT_LIMIT),
                i.to_string(),
                0.0,
                "s".repeat(TEXT_LIMIT)
            ));
            assert!(cache.bytes <= BYTE_LIMIT);
        }
        assert!(cache.entries.len() < ENTRY_LIMIT);
        assert!(cache.get("0", 0.0, &"s".repeat(TEXT_LIMIT)).is_none());
        assert!(
            cache
                .get(&(ENTRY_LIMIT - 1).to_string(), 0.0, &"s".repeat(TEXT_LIMIT))
                .is_some()
        );
    }
}
