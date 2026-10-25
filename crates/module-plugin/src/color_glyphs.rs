//! Native SVG and SVG-in-OpenType rasterization shared by preview and aux2.
use super::{AntialiasMethod, MAX_IMAGE_PIXELS, MAX_IMAGE_SIDE};
use windows::{
    Win32::{
        Foundation::{E_INVALIDARG, HMODULE, RECT},
        Graphics::{
            Direct2D::{Common::*, *},
            Direct3D::D3D_DRIVER_TYPE_WARP,
            Direct3D11::*,
            DirectWrite::{DWRITE_GLYPH_RUN, DWRITE_MEASURING_MODE},
            Dxgi::{Common::DXGI_FORMAT_B8G8R8A8_UNORM, IDXGIDevice},
        },
        UI::Shell::SHCreateMemStream,
    },
    core::{Interface, Result, w},
};
use windows_numerics::Matrix3x2;

pub(crate) struct SvgGlyphRenderer {
    context: ID2D1DeviceContext4,
}
impl SvgGlyphRenderer {
    pub fn new() -> Result<Self> {
        // WARP keeps rasterization independent of the host's D3D device and GPU.
        let mut device = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
        }
        let dxgi: IDXGIDevice = device.ok_or(E_INVALIDARG)?.cast()?;
        let factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        let device = unsafe { factory.CreateDevice(&dxgi) }?;
        let context =
            unsafe { device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE) }?.cast()?;
        Ok(Self { context })
    }

    pub fn draw_document(&mut self, xml: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
        let context: ID2D1DeviceContext5 = self.context.cast()?;
        unsafe {
            let stream = SHCreateMemStream(Some(xml)).ok_or(E_INVALIDARG)?;
            let document = context.CreateSvgDocument(
                &stream,
                D2D_SIZE_F {
                    width: width as f32,
                    height: height as f32,
                },
            )?;
            let root = document.GetRoot()?;
            // The output dimensions are the raster size, even for SVGs with
            // fixed width/height attributes. Retain their original coordinate
            // space as a viewBox before making the root fill the viewport.
            if !root.IsAttributeSpecified(w!("viewBox"), None).as_bool() {
                let mut dimensions = [D2D1_SVG_LENGTH::default(); 2];
                for (name, value) in [w!("width"), w!("height")].into_iter().zip(&mut dimensions) {
                    root.GetAttributeValue2(
                        name,
                        D2D1_SVG_ATTRIBUTE_POD_TYPE_LENGTH,
                        std::ptr::from_mut(value).cast(),
                        std::mem::size_of::<D2D1_SVG_LENGTH>() as u32,
                    )?;
                    if value.units != D2D1_SVG_LENGTH_UNITS_NUMBER
                        || !value.value.is_finite()
                        || value.value <= 0.0
                    {
                        return Err(E_INVALIDARG.into());
                    }
                }
                let view_box = D2D1_SVG_VIEWBOX {
                    x: 0.0,
                    y: 0.0,
                    width: dimensions[0].value,
                    height: dimensions[1].value,
                };
                root.SetAttributeValue2(
                    w!("viewBox"),
                    D2D1_SVG_ATTRIBUTE_POD_TYPE_VIEWBOX,
                    std::ptr::from_ref(&view_box).cast(),
                    std::mem::size_of::<D2D1_SVG_VIEWBOX>() as u32,
                )?;
            }
            root.SetAttributeValue3(w!("width"), D2D1_SVG_ATTRIBUTE_STRING_TYPE_SVG, w!("100%"))?;
            root.SetAttributeValue3(w!("height"), D2D1_SVG_ATTRIBUTE_STRING_TYPE_SVG, w!("100%"))?;
            let size = D2D_SIZE_U { width, height };
            let properties = bitmap_properties();
            let bitmap = context.CreateBitmap(size, None, 0, &properties)?;
            context.SetTarget(&bitmap);
            context.SetTransform(&Matrix3x2::identity());
            context.BeginDraw();
            context.Clear(None);
            context.DrawSvgDocument(&document);
            let result = context.EndDraw(None, None);
            context.SetTarget(None::<&ID2D1Image>);
            result?;
            read_bitmap(&self.context, &bitmap, size)
        }
    }

    pub fn draw(
        &mut self,
        run: &DWRITE_GLYPH_RUN,
        transform: Matrix3x2,
        mode: DWRITE_MEASURING_MODE,
        foreground: u32,
        antialias: AntialiasMethod,
    ) -> Result<(RECT, Vec<u8>)> {
        let context = &self.context;
        let foreground = D2D1_COLOR_F {
            r: ((foreground >> 16) & 255) as f32 / 255.0,
            g: ((foreground >> 8) & 255) as f32 / 255.0,
            b: (foreground & 255) as f32 / 255.0,
            a: 1.0,
        };
        unsafe {
            let brush = context.CreateSolidColorBrush(&foreground, None)?;
            let commands = context.CreateCommandList()?;
            context.SetTarget(&commands);
            context.SetTransform(&transform);
            context.SetAntialiasMode(if antialias == AntialiasMethod::Aliased {
                D2D1_ANTIALIAS_MODE_ALIASED
            } else {
                D2D1_ANTIALIAS_MODE_PER_PRIMITIVE
            });
            context.BeginDraw();
            context.DrawSvgGlyphRun(
                Default::default(),
                run,
                &brush,
                None::<&ID2D1SvgGlyphStyle>,
                0,
                mode,
            );
            let result = context.EndDraw(None, None);
            context.SetTarget(None::<&ID2D1Image>);
            result?;
            commands.Close()?;
            // The command list retains the SVG's true painted bounds, which
            // may differ from the fallback TrueType outline and font metrics.
            context.SetTransform(&Matrix3x2::identity());
            let ink = context.GetImageWorldBounds(&commands)?;
            if [ink.left, ink.top, ink.right, ink.bottom]
                .iter()
                .any(|v| !v.is_finite() || v.abs() > MAX_IMAGE_SIDE as f32)
            {
                return Err(E_INVALIDARG.into());
            }
            let bounds = RECT {
                left: ink.left.floor() as i32,
                top: ink.top.floor() as i32,
                right: ink.right.ceil() as i32,
                bottom: ink.bottom.ceil() as i32,
            };
            let width = bounds.right - bounds.left;
            let height = bounds.bottom - bounds.top;
            if width == 0 || height == 0 {
                return Ok((RECT::default(), Vec::new()));
            }
            if !(1..=MAX_IMAGE_SIDE).contains(&width)
                || !(1..=MAX_IMAGE_SIDE).contains(&height)
                || width as usize * height as usize > MAX_IMAGE_PIXELS
            {
                return Err(E_INVALIDARG.into());
            }
            let properties = bitmap_properties();
            let size = D2D_SIZE_U {
                width: width as u32,
                height: height as u32,
            };
            let bitmap = context.CreateBitmap(size, None, 0, &properties)?;
            context.SetTarget(&bitmap);
            context.SetTransform(&Matrix3x2 {
                M31: -bounds.left as f32,
                M32: -bounds.top as f32,
                ..Matrix3x2::identity()
            });
            context.BeginDraw();
            context.Clear(None);
            context.DrawImage(
                &commands,
                None,
                None,
                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR,
                D2D1_COMPOSITE_MODE_SOURCE_OVER,
            );
            let result = context.EndDraw(None, None);
            context.SetTarget(None::<&ID2D1Image>);
            result?;
            let bgra = read_bitmap(context, &bitmap, size)?;
            Ok((bounds, bgra))
        }
    }
}

fn bitmap_properties() -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
        ..Default::default()
    }
}

fn read_bitmap(
    context: &ID2D1DeviceContext4,
    bitmap: &ID2D1Bitmap1,
    size: D2D_SIZE_U,
) -> Result<Vec<u8>> {
    let properties = D2D1_BITMAP_PROPERTIES1 {
        bitmapOptions: D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
        ..bitmap_properties()
    };
    unsafe {
        let readback = context.CreateBitmap(size, None, 0, &properties)?;
        readback.CopyFromBitmap(None, bitmap, None)?;
        let mapped = readback.Map(D2D1_MAP_OPTIONS_READ)?;
        let stride = size.width as usize * 4;
        let mut bgra = vec![0; stride * size.height as usize];
        for y in 0..size.height as usize {
            // Map exposes rows with native pitch. Copy only initialized pixels.
            let row =
                std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), stride);
            bgra[y * stride..(y + 1) * stride].copy_from_slice(row);
        }
        readback.Unmap()?;
        for pixel in bgra.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            for value in &mut pixel[..3] {
                *value = (u32::from(*value) * 255 + alpha / 2)
                    .checked_div(alpha)
                    .unwrap_or(0)
                    .min(255) as u8;
            }
        }
        Ok(bgra)
    }
}
