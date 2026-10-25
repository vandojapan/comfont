mod egui_ui;
mod entrypoint;
mod font_collection;
mod model;
mod storage;
mod text_integration;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};

use aviutl2::generic::{GenericPlugin, SubPlugin};
use compositefont_core::ProfileDocument;
use compositefont_module::{CompositeFontModule, NativeTextFilter};
use windows::{
    Win32::Foundation::HWND,
    Win32::UI::WindowsAndMessaging::{IsWindow, PostQuitMessage},
};

fn host_window_exists(raw_hwnd: isize) -> bool {
    // SAFETY: IsWindow only inspects the borrowed window handle.
    unsafe { IsWindow(Some(HWND(raw_hwnd as *mut std::ffi::c_void))) }.as_bool()
}

struct RestoreHostQuit(isize);

impl Drop for RestoreHostQuit {
    fn drop(&mut self) {
        if !host_window_exists(self.0) {
            // The nested winit loop may have consumed the host's WM_QUIT while
            // dispatching thread-wide messages. Requeue it only after that loop
            // has returned so AviUtl2's outer message loop can terminate.
            // SAFETY: this callback and its nested eframe loop run on the host UI thread.
            unsafe { PostQuitMessage(0) };
        }
    }
}

/// 合成フォントプロファイルを編集する汎用プラグイン。
#[aviutl2::plugin(GenericPlugin)]
struct CompositeFontEditorPlugin {
    editor_runtime: Arc<Mutex<EditorRuntime>>,
    script_module: SubPlugin<CompositeFontModule>,
    native_text_filter: SubPlugin<NativeTextFilter>,
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
        let native_text_filter = SubPlugin::new_filter_plugin(&info)?;
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
            native_text_filter,
        })
    }

    fn plugin_info(&self) -> aviutl2::generic::GenericPluginTable {
        aviutl2::generic::GenericPluginTable {
            name: "Composite Font Editor".to_owned(),
            information: format!(
                "Composite Font profile editor for AviUtl2 / v{} β",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }

    fn register(&mut self, registry: &mut aviutl2::generic::HostAppHandle) {
        let edit_handle = Arc::new(registry.create_edit_handle());
        if let Ok(mut runtime) = self.editor_runtime.lock() {
            runtime.edit_handle = Some(Arc::clone(&edit_handle));
        } else {
            let _ = aviutl2::logger::write_error_log("合成フォント画面の共有状態を更新できません");
        }
        {
            let name = "合成フォント\\元の本文に戻す";
            let handle = Arc::clone(&edit_handle);
            registry.register_object_item_menu(name, move |object, effect, index, item| {
                if let Err(error) =
                    text_integration::restore_text(&handle, object, effect, index, item)
                {
                    let _ = aviutl2::logger::write_error_log(&error);
                }
            });
        }
        registry.register_script_module(Some("compositefont"), &self.script_module);
        registry.register_filter_plugin(&self.native_text_filter);
        if let Err(error) = compositefont_module::register_emoji_marker_font(registry) {
            let _ =
                aviutl2::logger::write_error_log(&format!("Comfont emoji marker font: {error}"));
        }
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
    #[config(name = "合成フォント (β)…")]
    fn show_config(owner: aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> {
        show_editor_menu(owner)
    }
}

fn show_editor_menu(owner: aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> {
    static OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if OPEN.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return Ok(());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            OPEN.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    let _guard = Guard;
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
            storage::load_document(&runtime.profile_path).map_err(aviutl2::anyhow::Error::msg)?,
            runtime.profile_path.clone(),
            edit_handle,
            Arc::clone(&runtime.font_registration),
        )
    };
    let host_window = edit_handle.get_host_app_window_raw().unwrap_or(owner);
    let _restore_host_quit = RestoreHostQuit(host_window.hwnd.get());
    let document = egui_ui::show_editor(
        host_window,
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

#[cfg(test)]
mod tests {
    use super::{CompositeFontEditorPlugin, RestoreHostQuit};
    use windows::Win32::UI::WindowsAndMessaging::{
        MSG, PM_NOREMOVE, PM_REMOVE, PeekMessageW, PostQuitMessage, WM_QUIT,
    };

    #[test]
    fn config_entry_does_not_borrow_the_generic_plugin_singleton() {
        let _: fn(aviutl2::Win32WindowHandle) -> aviutl2::AnyResult<()> =
            CompositeFontEditorPlugin::show_config;
    }

    #[test]
    fn restores_quit_message_consumed_by_a_nested_event_loop() {
        let mut message = MSG::default();
        // Create this test thread's message queue, then emulate winit removing
        // the host's WM_QUIT before the outer AviUtl2 loop can see it.
        unsafe {
            let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            PostQuitMessage(0);
            assert!(PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool());
        }
        drop(RestoreHostQuit(1)); // No real Win32 window uses this handle.
        unsafe {
            assert!(PeekMessageW(&mut message, None, WM_QUIT, WM_QUIT, PM_REMOVE).as_bool());
        }
        assert_eq!(message.message, WM_QUIT);
    }
}
