//! PNG export via headless vello.
//!
//! Builds a fresh scene from the document (no grid, no selection chrome,
//! no camera transform), renders it to an offscreen `Rgba8Unorm` texture
//! sized at the document's `viewBox` (or `width`/`height` fallback), reads
//! the pixels back to the CPU, and writes a PNG.
//!
//! Note: vello renders into linear `Rgba8Unorm`, not sRGB, so colours may
//! look slightly washed out compared to an sRGB framebuffer. This matches
//! the canvas viewport which has the same trade-off. Bit-exact sRGB export
//! lives in a later polish pass.

use std::path::Path;

use anyhow::{Result, anyhow};
use kurbo::{Affine, Vec2};
use vello::peniko::color::{AlphaColor, Srgb};
use vello::{AaConfig, RenderParams, Renderer, RendererOptions, Scene};
use youeye_doc::Document;

pub fn export_png(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    doc: &Document,
    path: &Path,
) -> Result<()> {
    let (min_x, min_y, doc_w, doc_h) = resolve_bounds(doc);
    if doc_w <= 0.0 || doc_h <= 0.0 {
        return Err(anyhow!(
            "document has no bounds — set a viewBox or explicit width/height"
        ));
    }

    let scale = 2.0_f64;
    let max_dim = device.limits().max_texture_dimension_2d;
    let w = ((doc_w * scale).ceil() as u32).clamp(1, max_dim);
    let h = ((doc_h * scale).ceil() as u32).clamp(1, max_dim);

    let mut scene = Scene::new();
    let xform = Affine::scale(scale) * Affine::translate(-Vec2::new(min_x, min_y));
    youeye_render::build(&mut scene, doc, xform);

    let mut renderer = Renderer::new(device, RendererOptions::default())
        .map_err(|e| anyhow!("vello renderer init: {e:?}"))?;

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("png export target"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    renderer
        .render_to_texture(
            device,
            queue,
            &scene,
            &view,
            &RenderParams {
                base_color: AlphaColor::<Srgb>::from_rgba8(0, 0, 0, 0),
                width: w,
                height: h,
                antialiasing_method: AaConfig::Area,
            },
        )
        .map_err(|e| anyhow!("vello render: {e:?}"))?;

    let bytes_per_pixel: u32 = 4;
    let unpadded_bpr = w * bytes_per_pixel;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bpr = unpadded_bpr.div_ceil(align) * align;
    let buffer_size = (padded_bpr as u64) * (h as u64);

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("png export staging"),
        size: buffer_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("png export encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| anyhow!("device poll: {e:?}"))?;
    rx.recv()
        .map_err(|e| anyhow!("map_async channel: {e:?}"))?
        .map_err(|e| anyhow!("map_async: {e:?}"))?;

    let data = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded_bpr as usize) * (h as usize));
    for row in 0..h {
        let start = (row * padded_bpr) as usize;
        let end = start + unpadded_bpr as usize;
        pixels.extend_from_slice(&data[start..end]);
    }
    drop(data);
    buffer.unmap();

    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut png_writer = encoder
        .write_header()
        .map_err(|e| anyhow!("png header: {e:?}"))?;
    png_writer
        .write_image_data(&pixels)
        .map_err(|e| anyhow!("png write: {e:?}"))?;

    Ok(())
}

fn resolve_bounds(doc: &Document) -> (f64, f64, f64, f64) {
    if let Some(vb) = doc.view_box {
        return (vb.min_x, vb.min_y, vb.width, vb.height);
    }
    let w = doc.width.unwrap_or(1024.0);
    let h = doc.height.unwrap_or(1024.0);
    (0.0, 0.0, w, h)
}
