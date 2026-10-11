//! Execute the shipped scripts with the same LuaJIT runtime as the test host.
#![cfg(windows)]

use std::{
    ffi::{c_char, c_int, c_void},
    path::PathBuf,
};

#[test]
#[ignore = "Requires the workspace AviUtl2 LuaJIT runtime"]
fn lua_generated_text_pipeline() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dll = std::env::var_os("COMFONT_LUAJIT_DLL")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".aviutl2-cli/development/luaJIT.dll"));
    unsafe {
        let library = libloading::Library::new(dll).expect("AviUtl2 LuaJIT runtime");
        let new_state = library
            .get::<unsafe extern "C" fn() -> *mut c_void>(b"luaL_newstate\0")
            .unwrap();
        let open_libs = library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"luaL_openlibs\0")
            .unwrap();
        let load = library
            .get::<unsafe extern "C" fn(*mut c_void, *const c_char, usize, *const c_char) -> c_int>(
                b"luaL_loadbuffer\0",
            )
            .unwrap();
        let call = library
            .get::<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int) -> c_int>(b"lua_pcall\0")
            .unwrap();
        let string = library
            .get::<unsafe extern "C" fn(*mut c_void, c_int, *mut usize) -> *const c_char>(
                b"lua_tolstring\0",
            )
            .unwrap();
        let close = library
            .get::<unsafe extern "C" fn(*mut c_void)>(b"lua_close\0")
            .unwrap();
        let state = new_state();
        assert!(!state.is_null());
        open_libs(state);
        let legacy = include_str!("../../../scripts/合成フォント.anm2");
        let bundle = include_str!("../../../scripts/@comfont.anm2");
        let (pipe, anm2) = bundle.split_once("\n@描画\n").unwrap();
        let pipe = pipe.strip_prefix("@パイプ\n").unwrap();
        let alias = include_str!("../../../scripts/合成フォント字幕.object");
        let script = format!(
            "anm2_source = [====[{anm2}]====]\nlegacy_anm2_source = [====[{legacy}]====]\npipe_source = [====[{pipe}]====]\nsubtitle_alias = [====[{alias}]====]\n{}",
            include_str!("../../../tests/lua/script_pipeline.lua")
        );
        let loaded = load(
            state,
            script.as_ptr().cast(),
            script.len(),
            c"comfont_pipeline".as_ptr(),
        );
        let result = if loaded == 0 {
            call(state, 0, 0, 0)
        } else {
            loaded
        };
        let error = if result != 0 {
            let mut len = 0;
            let ptr = string(state, -1, &mut len);
            if ptr.is_null() {
                "unknown Lua error".to_owned()
            } else {
                String::from_utf8_lossy(std::slice::from_raw_parts(ptr.cast::<u8>(), len))
                    .into_owned()
            }
        } else {
            String::new()
        };
        close(state);
        assert_eq!(result, 0, "{error}");
    }
}
