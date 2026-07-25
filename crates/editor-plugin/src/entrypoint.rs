use crate::{
    CompositeFontEditorPlugin,
    font_collection::{self, FontRegistrationOutcome},
};
use aviutl2::generic::__bridge::GenericSingleton;
use aviutl2::sys::{
    cache2::CACHE_HANDLE,
    config2::CONFIG_HANDLE,
    logger2::LOG_HANDLE,
    plugin2::{COMMON_PLUGIN_TABLE, HOST_APP_TABLE},
};

#[unsafe(no_mangle)]
unsafe extern "C" fn RequiredVersion() -> u32 {
    aviutl2::MINIMUM_AVIUTL2_VERSION.into()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn InitializeLogger(logger: *mut LOG_HANDLE) {
    aviutl2::logger::__initialize_logger_unwind(logger);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn InitializeConfig(config: *mut CONFIG_HANDLE) {
    aviutl2::config::__initialize_config_handle_unwind(config);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn InitializeCache(cache: *mut CACHE_HANDLE) {
    aviutl2::cache::__initialize_cache_unwind(cache);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn InitializePlugin(version: u32) -> bool {
    unsafe {
        aviutl2::generic::__bridge::initialize_plugin_c_unwind::<CompositeFontEditorPlugin>(version)
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn GetCommonPluginTable() -> *mut COMMON_PLUGIN_TABLE {
    unsafe { aviutl2::generic::__bridge::create_table_unwind::<CompositeFontEditorPlugin>() }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn RegisterPlugin(host: *mut HOST_APP_TABLE) {
    let result = std::panic::catch_unwind(|| unsafe { register_font_collection(host) });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = aviutl2::logger::write_error_log(&format!(
                "Composite Font: custom font collection registration failed: {error}"
            ));
        }
        Err(_) => {
            let _ = aviutl2::logger::write_error_log(
                "Composite Font: custom font collection registration panicked",
            );
        }
    }

    unsafe {
        aviutl2::generic::__bridge::register_plugin_unwind::<CompositeFontEditorPlugin>(host);
    }
}

unsafe fn register_font_collection(host: *mut HOST_APP_TABLE) -> Result<(), String> {
    let host = unsafe { host.as_ref() }.ok_or_else(|| "HOST_APP_TABLE is null".to_owned())?;
    let (outcome, collection_count) = CompositeFontEditorPlugin::with_instance_mut(|plugin| {
        let mut state = plugin
            .font_registration
            .lock()
            .map_err(|_| "font registration state is poisoned".to_owned())?;
        let outcome = state.register_initial_fonts(host.register_font_collection)?;
        Ok::<_, String>((outcome, state.registered_collection_count()))
    })?;

    log_registration_outcome(outcome, collection_count, "startup");
    Ok(())
}

pub(crate) fn log_registration_outcome(
    outcome: FontRegistrationOutcome,
    collection_count: usize,
    trigger: &str,
) {
    let message = match outcome {
        FontRegistrationOutcome::Added { font_file_count } => format!(
            "Composite Font: {trigger} registered {font_file_count} private font file(s) as additive collection #{collection_count} from {}",
            font_collection::font_directory().display()
        ),
        FontRegistrationOutcome::Unchanged => {
            format!("Composite Font: {trigger} found no private font changes; registration skipped")
        }
        FontRegistrationOutcome::NoFonts => format!(
            "Composite Font: {trigger} found no private font files in {}; existing registrations remain until restart",
            font_collection::font_directory().display()
        ),
    };
    let _ = aviutl2::logger::write_info_log(&message);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn UninitializePlugin() {
    unsafe {
        aviutl2::generic::__bridge::uninitialize_plugin_c_unwind::<CompositeFontEditorPlugin>();
    }
}
