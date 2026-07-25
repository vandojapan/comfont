mod model;
mod storage;
mod ui;

use std::path::PathBuf;

use aviutl2::generic::GenericPlugin;
use compositefont_core::ProfileDocument;

/// 合成フォントプロファイルを編集する汎用プラグイン。
#[aviutl2::plugin(GenericPlugin)]
struct CompositeFontEditorPlugin {
    document: ProfileDocument,
    profile_path: PathBuf,
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
        registry.register_menus::<Self>();
    }
}

#[aviutl2::generic::menus]
impl CompositeFontEditorPlugin {
    #[config(name = "合成フォント…")]
    fn show_config(&mut self, owner: aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> {
        self.document = ui::show_editor(owner, self.document.clone(), self.profile_path.clone())
            .map_err(aviutl2::anyhow::Error::msg)?;
        Ok(())
    }
}

aviutl2::register_generic_plugin!(CompositeFontEditorPlugin);
