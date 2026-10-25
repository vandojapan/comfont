//! Optional aux2: SVG rasterization only. Layout and image composition belong
//! to the caller; this plugin never modifies standard text or project data.
use aviutl2::{
    AnyResult,
    generic::{GenericPlugin, GenericPluginTable, HostAppHandle, SubPlugin},
    module::{ScriptModule, ScriptModuleFunctions, ScriptModuleTable},
};
use compositefont_module::raster::{self, RasterImage};
use std::{collections::HashMap, io::BufWriter, path::PathBuf, sync::Mutex};

#[derive(Clone, Hash, PartialEq, Eq)]
enum Request {
    Svg(String, u32, u32),
    Glyph(String, String, u64),
}
impl Request {
    fn bytes(&self) -> usize {
        match self {
            Self::Svg(xml, _, _) => xml.len(),
            Self::Glyph(text, font, _) => text.len() + font.len(),
        }
    }
}

#[derive(Clone)]
struct CachedImage {
    path: PathBuf,
    width: u32,
    height: u32,
    origin_x: f32,
    baseline: f32,
    advance: f32,
}
struct ImageCache {
    // Session-local files cannot become stale after a font/plugin update.
    // Keep them alive until unload: AviUtl2 may still refer to returned paths.
    directory: tempfile::TempDir,
    images: HashMap<Request, CachedImage>,
    bytes: usize,
}
impl ImageCache {
    fn new() -> std::io::Result<Self> {
        Ok(Self {
            directory: tempfile::Builder::new().prefix("comfont-svg-").tempdir()?,
            images: HashMap::new(),
            bytes: 0,
        })
    }
    fn insert(&mut self, key: Request, image: RasterImage) -> Result<CachedImage, String> {
        if let Some(cached) = self.images.get(&key) {
            return Ok(cached.clone());
        }
        let mut rgba = image.bgra;
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        let mut encoded = Vec::new();
        {
            let mut encoder =
                png::Encoder::new(BufWriter::new(&mut encoded), image.width, image.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
            writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
            writer.finish().map_err(|e| e.to_string())?;
        }
        let bytes = encoded.len() + key.bytes();
        if self.images.len() >= 1024 || self.bytes.saturating_add(bytes) > 64 * 1024 * 1024 {
            return Err("SVG session cache limit reached (1024 images / 64 MiB)".into());
        }
        let path = self
            .directory
            .path()
            .join(format!("{}.png", self.images.len()));
        std::fs::write(&path, encoded).map_err(|e| e.to_string())?;
        let result = CachedImage {
            path,
            width: image.width,
            height: image.height,
            origin_x: image.origin_x,
            baseline: image.baseline,
            advance: image.advance,
        };
        self.bytes += bytes;
        self.images.insert(key, result.clone());
        Ok(result)
    }
}

// path, width, height, origin_x, baseline, advance, error; failure returns nil
// for path and a diagnostic string in the last position.
type ImageResult = (Option<String>, f64, f64, f64, f64, f64, Option<String>);
fn result(value: Result<CachedImage, String>) -> ImageResult {
    match value {
        Ok(image) => (
            Some(image.path.to_string_lossy().into_owned()),
            image.width.into(),
            image.height.into(),
            image.origin_x.into(),
            image.baseline.into(),
            image.advance.into(),
            None,
        ),
        Err(error) => (None, 0.0, 0.0, 0.0, 0.0, 0.0, Some(error)),
    }
}

#[aviutl2::plugin(ScriptModule)]
struct SvgModule {
    cache: Mutex<ImageCache>,
}
impl SvgModule {
    fn cached(
        &self,
        key: Request,
        render: impl FnOnce() -> Result<RasterImage, String>,
    ) -> ImageResult {
        let cached = self
            .cache
            .lock()
            .map_err(|_| "SVG cache lock failed".to_owned())
            .map(|cache| cache.images.get(&key).cloned());
        match cached {
            Ok(Some(image)) => return result(Ok(image)),
            Err(error) => return result(Err(error)),
            Ok(None) => (),
        }
        // FontManager may call host code. Never hold the cache lock across it.
        result(render().and_then(|image| {
            self.cache
                .lock()
                .map_err(|_| "SVG cache lock failed".to_owned())?
                .insert(key, image)
        }))
    }
}
impl ScriptModule for SvgModule {
    fn new(_: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        Ok(Self {
            cache: Mutex::new(ImageCache::new()?),
        })
    }
    fn plugin_info(&self) -> ScriptModuleTable {
        ScriptModuleTable {
            information: format!(
                "Comfont SVG rasterizer / v{} / API 1",
                env!("CARGO_PKG_VERSION")
            ),
            functions: Self::functions(),
        }
    }
}

#[aviutl2::module::functions]
impl SvgModule {
    fn api_version(&self) -> i32 {
        1
    }

    fn rasterize_svg(&self, xml: String, width: f64, height: f64) -> ImageResult {
        if !width.is_finite()
            || !height.is_finite()
            || width.fract() != 0.0
            || height.fract() != 0.0
            || !(1.0..=8192.0).contains(&width)
            || !(1.0..=8192.0).contains(&height)
            || width * height > 16_777_216.0
            || xml.len() > 1024 * 1024
        {
            return result(Err("Invalid SVG XML or integer pixel dimensions".into()));
        }
        let key = Request::Svg(xml.clone(), width as u32, height as u32);
        self.cached(key, || raster::svg(&xml, width as u32, height as u32))
    }

    fn rasterize_glyph(
        &self,
        text: String,
        font: String,
        size: f64,
        section: &aviutl2::generic::ReadSection,
    ) -> ImageResult {
        let key = Request::Glyph(text.clone(), font.clone(), size.to_bits());
        self.cached(key, || raster::glyph(section, &text, &font, size))
    }
}

#[aviutl2::plugin(GenericPlugin)]
struct SvgPlugin {
    module: SubPlugin<SvgModule>,
}
impl GenericPlugin for SvgPlugin {
    fn new(info: aviutl2::AviUtl2Info) -> AnyResult<Self> {
        Ok(Self {
            module: SubPlugin::new_script_module(&info)?,
        })
    }
    fn plugin_info(&self) -> GenericPluginTable {
        GenericPluginTable {
            name: "Comfont SVG rasterizer".into(),
            information: "SVG image-producing module for Lua".into(),
        }
    }
    fn register(&mut self, host: &mut HostAppHandle) {
        host.register_script_module(Some("comfont_svg"), &self.module);
    }
}
aviutl2::register_generic_plugin!(SvgPlugin);

#[cfg(test)]
mod tests {
    use super::*;
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect x="2" y="2" width="6" height="6" fill="#ff0000" fill-opacity="0.5"/></svg>"##;
    #[test]
    fn sized_raster_cache_and_png_alpha() {
        let mut cache = ImageCache::new().unwrap();
        for size in [16, 64, 128] {
            let image = raster::svg(SVG, size, size).unwrap();
            assert_eq!(image.bgra.len(), (size * size * 4) as usize);
            assert_eq!(&image.bgra[..4], &[0, 0, 0, 0]);
            let center = ((size / 2 * size + size / 2) * 4) as usize;
            let pixel = &image.bgra[center..center + 4];
            assert_eq!(&pixel[..3], &[0, 0, 255]);
            assert!((127..=128).contains(&pixel[3]));
            let key = Request::Svg(SVG.into(), size, size);
            let saved = cache.insert(key.clone(), image).unwrap();
            let mut decoded = png::Decoder::new(std::io::BufReader::new(
                std::fs::File::open(&saved.path).unwrap(),
            ))
            .read_info()
            .unwrap();
            assert_eq!((decoded.info().width, decoded.info().height), (size, size));
            assert_eq!(decoded.info().color_type, png::ColorType::Rgba);
            let mut pixels = vec![0; decoded.output_buffer_size().unwrap()];
            decoded.next_frame(&mut pixels).unwrap();
            assert_eq!(&pixels[center..center + 3], &[255, 0, 0]);
            assert!((127..=128).contains(&pixels[center + 3]));
            let again = cache
                .insert(key, raster::svg(SVG, size, size).unwrap())
                .unwrap();
            assert_eq!(saved.path, again.path);
        }
        assert_eq!(cache.images.len(), 3);
    }
    #[test]
    fn fixed_svg_dimensions_scale_from_vectors() {
        for source in [
            SVG.replace("viewBox=\"0 0 10 10\"", "width=\"10\" height=\"10\""),
            SVG.replace(
                "viewBox=\"0 0 10 10\"",
                "width=\"10\" height=\"10\" viewBox=\"0 0 10 10\"",
            ),
        ] {
            let image = raster::svg(&source, 128, 128).unwrap();
            let pixel = &image.bgra[(64 * 128 + 64) * 4..][..4];
            assert_eq!(&pixel[..3], &[0, 0, 255]);
            assert!((127..=128).contains(&pixel[3]));
        }
    }
    #[test]
    fn invalid_svg_and_sizes_fail_without_a_cached_image() {
        for (xml, w, h) in [
            ("not XML", 64, 64),
            (SVG, 0, 64),
            (SVG, 8193, 1),
            (SVG, 8192, 8192),
        ] {
            assert!(raster::svg(xml, w, h).is_err());
        }
    }
}
