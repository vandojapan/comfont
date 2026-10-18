//! Isolated SDK experiment. Never included in the Comfont package.
//! Run only in the dedicated, empty v2.1.14 test host named below.
use aviutl2::{
    AnyResult,
    filter::{
        FilterConfigItem, FilterPlugin, FilterPluginFlags, FilterPluginTable, FilterProcVideo,
        ImageResource, OutputImageResourcePixelFormat,
    },
    generic::{
        EditHandle, GenericPlugin, GenericPluginTable, HostAppHandle, ProjectFile, SubPlugin,
    },
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const HOST: &str = "comfont-native-text-verify-20261018";
const EFFECT: &str = "Comfont native text probe";
const SCRIPT_EFFECT: &str = "描画@comfont-native-probe";
const ORIGINAL: &str = "原文 ABC ffi";
const DECORATED: &str = "<s96,Arial><#ff0000>ABC ffi<#><s>";
static PIXELS_VERIFIED: AtomicBool = AtomicBool::new(false);

fn root() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}
fn log(message: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root().join("native-text-probe.log"))
    {
        let _ = writeln!(file, "{message}");
    }
}
fn parameters(text: &str) -> Vec<(String, String)> {
    [
        ("サイズ", "64.00"),
        ("字間", "0.00"),
        ("行間", "0.00"),
        ("表示速度", "0.00"),
        ("フォント", "Yu Gothic UI"),
        ("文字色", "ffffff"),
        ("影・縁色", "000000"),
        ("文字装飾", "標準文字"),
        ("文字揃え", "左寄せ[上]"),
        ("B", "0"),
        ("I", "0"),
        ("テキスト", text),
        ("文字毎に個別オブジェクト", "0"),
        ("自動スクロール", "0"),
        ("移動座標上に表示", "0"),
        ("オブジェクトの長さを自動調節", "0"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}
fn alias(text: &str) -> String {
    let mut value = "[Object]\n[Object.0]\neffect.name=テキスト\n".to_owned();
    for (k, v) in parameters(text) {
        value.push_str(&format!("{k}={v}\n"));
    }
    value
}

#[aviutl2::plugin(FilterPlugin)]
struct NativeText;
impl FilterPlugin for NativeText {
    type Userdata = ();
    fn new(_: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        Ok(Self)
    }
    fn plugin_info(&self) -> FilterPluginTable {
        FilterPluginTable {
            name: EFFECT.into(),
            label: None,
            information: "SDK exec_effect text experiment; not a production renderer".into(),
            flags: aviutl2::bitflag!(FilterPluginFlags {
                video: true,
                hidemenu: true
            }),
            config_items: vec![],
        }
    }
    fn proc_video(&self, _: &[FilterConfigItem], video: &mut FilterProcVideo<()>) -> AnyResult<()> {
        let object = video.object.handle;
        let profile = video.read_section().get_object_effect_item(
            object,
            SCRIPT_EFFECT,
            0,
            "comfont::プロファイル",
        )?;
        if profile != "probe" {
            return Ok(());
        }
        let before =
            video
                .read_section()
                .get_object_effect_item(object, "テキスト", 0, "テキスト")?;
        let params = parameters(DECORATED);
        let result = video.exec_effect(
            "テキスト",
            params.iter().map(|(k, v)| (k, v)),
            &ImageResource::Object,
        );
        let after =
            video
                .read_section()
                .get_object_effect_item(object, "テキスト", 0, "テキスト")?;
        log(&format!(
            "profile={profile}; exec_effect(text)={result:?}; source_unchanged={}; size={:?}",
            before == after,
            video.get_image_resource_size(&ImageResource::Object)
        ));
        aviutl2::anyhow::ensure!(
            before == after,
            "native input invocation changed saved source"
        );
        result?;
        if !PIXELS_VERIFIED.load(Ordering::Acquire) {
            let expected = ImageResource::Resource("comfont-native-reference".into());
            let copied = video.copy_image_resource(
                &ImageResource::Layer {
                    layer: 0,
                    apply_additional_effects: false,
                },
                &expected,
            );
            log(&format!("copy reference layer: {copied:?}"));
            copied?;
            let (width, height) = video.get_image_resource_size(&ImageResource::Object)?;
            let reference_size = video.get_image_resource_size(&expected)?;
            aviutl2::anyhow::ensure!(
                reference_size == (width, height),
                "native reference dimensions differ: {reference_size:?}"
            );
            let mut actual = vec![0; width as usize * height as usize * 4];
            let mut reference = actual.clone();
            // Readback is an experiment assertion only; these bytes are never uploaded.
            video.get_image_resource_data(
                &ImageResource::Object,
                &mut actual,
                width,
                height,
                width * 4,
                OutputImageResourcePixelFormat::Rgba,
            )?;
            video.get_image_resource_data(
                &expected,
                &mut reference,
                width,
                height,
                width * 4,
                OutputImageResourcePixelFormat::Rgba,
            )?;
            video.release_image_resource(&expected)?;
            let visible = actual.chunks_exact(4).any(|px| px[3] != 0);
            log(&format!(
                "native reference: size={reference_size:?}; visible={visible}; identical_pixels={}",
                actual == reference
            ));
            aviutl2::anyhow::ensure!(
                visible && actual == reference,
                "native reference pixels differ"
            );
            PIXELS_VERIFIED.store(true, Ordering::Release);
        }
        Ok(())
    }
}

#[aviutl2::plugin(GenericPlugin)]
struct Probe {
    filter: SubPlugin<NativeText>,
    handle: Option<Arc<EditHandle>>,
    started: AtomicBool,
}
impl GenericPlugin for Probe {
    fn new(info: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        Ok(Self {
            filter: SubPlugin::new_filter_plugin(&info)?,
            handle: None,
            started: AtomicBool::new(false),
        })
    }
    fn plugin_info(&self) -> GenericPluginTable {
        GenericPluginTable {
            name: "Comfont native text experiment".into(),
            information: "Creates only temporary test objects in a dedicated empty test host"
                .into(),
        }
    }
    fn register(&mut self, host: &mut HostAppHandle) {
        self.handle = Some(Arc::new(host.create_edit_handle()));
        host.register_filter_plugin(&self.filter);
    }
    fn on_project_load(&mut self, _: &mut ProjectFile) {
        if root().file_name().and_then(|s| s.to_str()) != Some(HOST)
            || self.started.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let handle = self.handle.clone().unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1000));
            if let Err(error) = verify(&handle) {
                log(&format!("FAIL: {error:#}"));
            }
        });
    }
}
aviutl2::register_generic_plugin!(Probe);

fn verify(handle: &EditHandle) -> AnyResult<()> {
    log(
        "START: native standard text via exec_effect; readback only for comparison, no pixel upload",
    );
    let test = handle.call_edit_section(|section| -> AnyResult<_> {
        aviutl2::anyhow::ensure!(
            section.get_focused_object()?.is_none(),
            "test host must be empty"
        );
        section.create_object_from_alias(&alias(DECORATED), 0, 0, 60)?;
        let test = section.create_object_from_alias(&alias(ORIGINAL), 1, 0, 60)?;
        let effect = section.create_effect(test, SCRIPT_EFFECT)?;
        section.set_effect_item_value(effect, "comfont::プロファイル", "probe")?;
        section.set_focus_object(Some(test))?;
        Ok(test)
    })??;
    for _ in 0..100 {
        if PIXELS_VERIFIED.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let preserved = handle.call_read_section(move |section| {
        section.get_object_effect_item(test, "テキスト", 0, "テキスト")
    })??;
    log(&format!("preserved_source={preserved:?}"));
    aviutl2::anyhow::ensure!(preserved == ORIGINAL, "saved body changed");
    aviutl2::anyhow::ensure!(
        PIXELS_VERIFIED.load(Ordering::Acquire),
        "native pixel comparison did not pass"
    );
    handle.save_project_file(&root().join("native-text-proof.aup2"))?;
    log("PASS: identical native output and unchanged standard text source");
    Ok(())
}
