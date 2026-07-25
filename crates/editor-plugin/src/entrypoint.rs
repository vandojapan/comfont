use crate::CompositeFontEditorPlugin;
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
    unsafe {
        aviutl2::generic::__bridge::register_plugin_unwind::<CompositeFontEditorPlugin>(host);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn UninitializePlugin() {
    unsafe {
        aviutl2::generic::__bridge::uninitialize_plugin_c_unwind::<CompositeFontEditorPlugin>();
    }
}
