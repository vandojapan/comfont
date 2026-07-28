mod entrypoint;
mod font_collection;
mod model;
mod storage;
mod ui;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use aviutl2::generic::{GenericPlugin, SubPlugin};
use compositefont_core::ProfileDocument;
use compositefont_module::CompositeFontModule;

/// 合成フォントプロファイルを編集する汎用プラグイン。
#[aviutl2::plugin(GenericPlugin)]
struct CompositeFontEditorPlugin {
    editor_runtime: Arc<Mutex<EditorRuntime>>,
    script_module: SubPlugin<CompositeFontModule>,
}

struct EditorRuntime {
    document: ProfileDocument,
    profile_path: PathBuf,
    font_registration: Arc<Mutex<font_collection::FontRegistrationState>>,
    edit_handle: Option<Arc<aviutl2::generic::EditHandle>>,
}

static EDITOR_RUNTIME: Mutex<Option<Weak<Mutex<EditorRuntime>>>> = Mutex::new(None);

fn install_editor_runtime(runtime: &Arc<Mutex<EditorRuntime>>) -> Result<(), String> {
    let mut shared = EDITOR_RUNTIME
        .lock()
        .map_err(|_| "合成フォント画面の共有状態を初期化できません".to_owned())?;
    *shared = Some(Arc::downgrade(runtime));
    Ok(())
}

fn editor_runtime() -> Result<Arc<Mutex<EditorRuntime>>, String> {
    EDITOR_RUNTIME
        .lock()
        .map_err(|_| "合成フォント画面の共有状態を取得できません".to_owned())?
        .as_ref()
        .and_then(Weak::upgrade)
        .ok_or_else(|| "合成フォント画面は初期化されていません".to_owned())
}

impl GenericPlugin for CompositeFontEditorPlugin {
    fn new(info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
        let profile_path = storage::default_profile_path();
        let document = storage::load_document(&profile_path).unwrap_or_else(|error| {
            aviutl2::logger::write_error_log(&error).ok();
            ProfileDocument::with_builtin_default()
        });
        let script_module = SubPlugin::new_script_module(&info)?;
        let editor_runtime = Arc::new(Mutex::new(EditorRuntime {
            document,
            profile_path,
            font_registration: Arc::new(Mutex::new(font_collection::FontRegistrationState)),
            edit_handle: None,
        }));
        install_editor_runtime(&editor_runtime).map_err(aviutl2::anyhow::Error::msg)?;
        Ok(Self {
            editor_runtime,
            script_module,
        })
    }

    fn plugin_info(&self) -> aviutl2::generic::GenericPluginTable {
        aviutl2::generic::GenericPluginTable {
            name: "Composite Font Editor".to_owned(),
            information: format!(
                "Composite Font profile editor for AviUtl2 / v{}",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }

    fn register(&mut self, registry: &mut aviutl2::generic::HostAppHandle) {
        let edit_handle = Arc::new(registry.create_edit_handle());
        if let Ok(mut runtime) = self.editor_runtime.lock() {
            runtime.edit_handle = Some(edit_handle);
        } else {
            let _ = aviutl2::logger::write_error_log("合成フォント画面の共有状態を更新できません");
        }
        registry.register_script_module(Some("compositefont"), &self.script_module);
        registry.register_menus::<Self>();
    }

    fn on_project_load(&mut self, _project: &mut aviutl2::generic::ProjectFile) {
        let edit_handle = self
            .editor_runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.edit_handle.clone());
        let Some(edit_handle) = edit_handle else {
            return;
        };
        let font_names = edit_handle.get_font_names();
        let _ = aviutl2::logger::write_info_log(&format!(
            "Composite Font: FontManager exposes {} font name(s)",
            font_names.len()
        ));
    }
}

#[aviutl2::generic::menus]
impl CompositeFontEditorPlugin {
    #[config(name = "合成フォント…")]
    fn show_config(owner: aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> {
        // This menu callback must not borrow the GenericPlugin singleton. The
        // editor runs a nested modal message loop, while AviUtl2 event threads
        // continue entering GenericPlugin callbacks.
        let runtime = editor_runtime().map_err(aviutl2::anyhow::Error::msg)?;
        let (document, profile_path, edit_handle, font_registration) = {
            let runtime = runtime
                .lock()
                .map_err(|_| aviutl2::anyhow::Error::msg("Editor state lock was poisoned"))?;
            let edit_handle = runtime
                .edit_handle
                .clone()
                .ok_or_else(|| aviutl2::anyhow::Error::msg("EditHandle is not initialized"))?;
            (
                runtime.document.clone(),
                runtime.profile_path.clone(),
                edit_handle,
                Arc::clone(&runtime.font_registration),
            )
        };
        let document = ui::show_editor(
            owner,
            document,
            profile_path,
            edit_handle,
            font_registration,
        )
        .map_err(aviutl2::anyhow::Error::msg)?;
        runtime
            .lock()
            .map_err(|_| aviutl2::anyhow::Error::msg("Editor state lock was poisoned"))?
            .document = document;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::CompositeFontEditorPlugin;

    #[test]
    fn config_entry_does_not_borrow_the_generic_plugin_singleton() {
        let _: fn(aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> =
            CompositeFontEditorPlugin::show_config;
    }
}
