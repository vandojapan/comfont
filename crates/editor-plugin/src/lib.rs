mod entrypoint;
mod font_collection;
mod model;
mod storage;
mod ui;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use aviutl2::generic::GenericPlugin;
use compositefont_core::ProfileDocument;

/// 合成フォントプロファイルを編集する汎用プラグイン。
#[aviutl2::plugin(GenericPlugin)]
struct CompositeFontEditorPlugin {
    document: ProfileDocument,
    profile_path: PathBuf,
    font_registration: Arc<Mutex<font_collection::FontRegistrationState>>,
    edit_handle: Option<Arc<aviutl2::generic::EditHandle>>,
}

impl GenericPlugin for CompositeFontEditorPlugin {
    fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
        let profile_path = storage::default_profile_path();
        let document = storage::load_document(&profile_path).unwrap_or_else(|error| {
            aviutl2::logger::write_error_log(&error).ok();
            ProfileDocument::with_builtin_default()
        });
        Ok(Self {
            document,
            profile_path,
            font_registration: Arc::new(Mutex::new(
                font_collection::FontRegistrationState::default(),
            )),
            edit_handle: None,
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
        self.edit_handle = Some(Arc::new(registry.create_edit_handle()));
        registry.register_menus::<Self>();
    }

    fn on_project_load(&mut self, _project: &mut aviutl2::generic::ProjectFile) {
        let Some(edit_handle) = &self.edit_handle else {
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
    fn show_config(&mut self, owner: aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> {
        let edit_handle = self
            .edit_handle
            .as_ref()
            .cloned()
            .ok_or_else(|| aviutl2::anyhow::Error::msg("EditHandle is not initialized"))?;
        self.document = ui::show_editor(
            owner,
            self.document.clone(),
            self.profile_path.clone(),
            edit_handle,
            Arc::clone(&self.font_registration),
        )
        .map_err(aviutl2::anyhow::Error::msg)?;
        Ok(())
    }
}
