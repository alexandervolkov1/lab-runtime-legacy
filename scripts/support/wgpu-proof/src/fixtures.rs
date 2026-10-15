//! One bounded deterministic egui/egui_plot fixture for both real renderers.
use crate::{Result, write};
use eframe::egui::{self, Color32, TextureId};
use egui_plot::{Line, Plot, PlotPoint};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(super) const WIDTH: u32 = 1280;
pub(super) const HEIGHT: u32 = 960;
pub(super) const READBACK: u64 = WIDTH as u64 * HEIGHT as u64 * 4;
pub(super) const FRAMES: usize = 12;
pub(super) const CYAN: Color32 = Color32::from_rgb(0, 220, 220);
pub(super) const MAGENTA: Color32 = Color32::from_rgb(230, 30, 170);

pub(super) struct Fixture {
    pub ctx: egui::Context,
    image: Option<egui::TextureHandle>,
}
pub(super) struct Frame {
    pub output: egui::FullOutput,
    pub meshes: Vec<egui::ClippedPrimitive>,
    pub receipt: Value,
    pub index: usize,
}
impl Fixture {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        ctx.set_visuals(egui::Visuals::dark());
        ctx.options_mut(|o| {
            o.max_passes = std::num::NonZeroUsize::new(1).unwrap();
            o.repaint_on_widget_change = false;
        });
        Self { ctx, image: None }
    }
    pub fn frame(&mut self, index: usize) -> Frame {
        assert!(index < FRAMES);
        match index {
            6 | 10 => {
                self.image = Some(self.ctx.load_texture(
                    "bounded",
                    egui::ColorImage::filled([64, 64], Color32::RED),
                    egui::TextureOptions::NEAREST,
                ))
            }
            7 => self.image.as_mut().unwrap().set_partial(
                [16, 16],
                egui::ColorImage::filled([32, 32], Color32::GREEN),
                egui::TextureOptions::NEAREST,
            ),
            8 => self.image.as_mut().unwrap().set(
                egui::ColorImage::filled([512, 512], Color32::GREEN),
                egui::TextureOptions::LINEAR,
            ),
            9 => self.image = None,
            _ => {}
        }
        let title = if index >= 5 {
            "Replacement: voltage history"
        } else {
            "RENDER-2A read-only fixture"
        };
        let scalar = match index {
            0 => "No data",
            1..=3 => "23.50 degC",
            _ => "27.25 degC",
        };
        let points = if index == 11 { 8192 } else { 65 };
        let mut sample = egui::Pos2::ZERO;
        let mut scalar_rect = egui::Rect::NOTHING;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH as f32, HEIGHT as f32),
            )),
            time: Some(index as f64 / 60.0),
            max_texture_side: Some(2048),
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(1.0);
        let mut output = self.ctx.run_ui(input, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(18, 18, 22))
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(1256.0, 936.0));
                    ui.heading(title);
                    scalar_rect = ui
                        .label(egui::RichText::new(scalar).size(30.0).color(Color32::WHITE))
                        .rect;
                    let plot = Plot::new("same-plot")
                        .width(1000.0)
                        .height(740.0)
                        .include_x(0.0)
                        .include_x(64.0)
                        .include_y(-2.0)
                        .include_y(2.0)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .x_axis_label(if index >= 5 {
                            "elapsed / ms"
                        } else {
                            "time / s"
                        })
                        .y_axis_label(if index >= 5 {
                            "voltage / V"
                        } else {
                            "temperature / degC"
                        })
                        .show(ui, |plot| {
                            if index >= 2 {
                                let offset = if index >= 4 { 0.5 } else { 0.0 };
                                let values: Vec<[f64; 2]> = (0..points)
                                    .map(|i| {
                                        let x = i as f64 * 64.0 / (points - 1) as f64;
                                        [x, (x / 8.0).sin() + offset]
                                    })
                                    .collect();
                                plot.line(Line::new("channel A", values).color(CYAN).width(3.0));
                            }
                            if index >= 3 {
                                let values: Vec<[f64; 2]> = (0..points)
                                    .map(|i| {
                                        let x = i as f64 * 64.0 / (points - 1) as f64;
                                        [x, (x / 8.0).cos() - 0.5]
                                    })
                                    .collect();
                                plot.line(Line::new("channel B", values).color(MAGENTA).width(3.0));
                            }
                        });
                    sample = plot.transform.position_from_point(&PlotPoint::new(
                        32.0,
                        (4.0_f64).sin() + if index >= 4 { 0.5 } else { 0.0 },
                    ));
                    if let Some(image) = &self.image {
                        ui.painter().image(
                            image.id(),
                            egui::Rect::from_min_max(
                                egui::pos2(1100.0, 100.0),
                                egui::pos2(1228.0, 228.0),
                            ),
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                });
        });
        let meshes = self
            .ctx
            .tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        let mut vertices = 0;
        let mut indices = 0;
        let mut hash = 0xcbf29ce484222325_u64;
        let mut feed = |bytes: &[u8]| {
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
            }
        };
        for primitive in &meshes {
            for value in [
                primitive.clip_rect.min.x,
                primitive.clip_rect.min.y,
                primitive.clip_rect.max.x,
                primitive.clip_rect.max.y,
            ] {
                feed(&value.to_bits().to_le_bytes());
            }
            if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                feed(format!("{:?}", mesh.texture_id).as_bytes());
                vertices += mesh.vertices.len();
                indices += mesh.indices.len();
                for v in &mesh.vertices {
                    for value in [v.pos.x, v.pos.y, v.uv.x, v.uv.y] {
                        feed(&value.to_bits().to_le_bytes());
                    }
                    feed(&v.color.to_array());
                }
                for i in &mesh.indices {
                    feed(&i.to_le_bytes());
                }
            }
        }
        let updates: Vec<_> = sorted_updates(&output).iter().map(|(id, delta)| json!({"id":format!("{id:?}"),"size":delta.image.size(),"position":delta.pos,"options":format!("{:?}",delta.options)})).collect();
        let receipt = json!({"frame":index,"title":title,"scalar":scalar,"sample":[sample.x,sample.y],"scalar_rect":[scalar_rect.min.x,scalar_rect.min.y,scalar_rect.max.x,scalar_rect.max.y],"vertices":vertices,"indices":indices,"vertex_bytes":vertices*20,"index_bytes":indices*4,"primitives":meshes.len(),"mesh_fnv64":format!("{hash:016x}"),"updates":updates,"free":format!("{:?}",output.textures_delta.free),"texture_id":self.image.as_ref().map(|t| format!("{:?}",t.id()))});
        Frame {
            output,
            meshes,
            receipt,
            index,
        }
    }
}

pub(super) fn sorted_updates(
    output: &egui::FullOutput,
) -> Vec<(TextureId, &egui::epaint::ImageDelta)> {
    let mut ids: Vec<_> = output.textures_delta.set.keys().copied().collect();
    ids.sort_unstable();
    ids.into_iter()
        .flat_map(|id| {
            output.textures_delta.set[&id]
                .iter()
                .map(move |delta| (id, delta))
        })
        .collect()
}

#[derive(Default)]
pub(super) struct Bounds {
    pub textures: BTreeMap<TextureId, [usize; 2]>,
}
impl Bounds {
    // Stage metadata before any GPU call, publish only after the full CPU check.
    pub fn validate(&mut self, frame: &Frame) -> Result<()> {
        if frame.meshes.len() > 192 || frame.output.pixels_per_point != 1.0 {
            return Err("frame bounds".into());
        }
        let mut staged = self.textures.clone();
        let updates = sorted_updates(&frame.output);
        if updates.len() > 16 || frame.output.textures_delta.free.len() > 8 {
            return Err("delta bounds".into());
        }
        for (id, delta) in updates {
            let size = delta.image.size();
            if !matches!(id, TextureId::Managed(_))
                || size.contains(&0)
                || size[0].checked_mul(size[1]).is_none_or(|n| n > 1024 * 1024)
                || delta.options.mipmap_mode.is_some()
            {
                return Err("image bounds".into());
            }
            let egui::ImageData::Color(image) = &delta.image;
            if image.pixels.len() != size[0] * size[1] {
                return Err("image length".into());
            }
            if let Some(pos) = delta.pos {
                let old = staged.get(&id).ok_or("partial missing texture")?;
                if (0..2).any(|axis| {
                    pos[axis]
                        .checked_add(size[axis])
                        .is_none_or(|end| end > old[axis])
                }) {
                    return Err("partial bounds".into());
                }
            } else {
                staged.insert(id, size);
            }
            if staged.len() > 8 {
                return Err("texture slots".into());
            }
        }
        let mut vb = 0;
        let mut ib = 0;
        for primitive in &frame.meshes {
            let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                return Err("callback forbidden".into());
            };
            vb += mesh.vertices.len() * 20;
            ib += mesh.indices.len() * 4;
            if vb > 2 * 1024 * 1024
                || ib > 2 * 1024 * 1024
                || !staged.contains_key(&mesh.texture_id)
                || !mesh.is_valid()
                || mesh.vertices.iter().any(|v| {
                    !v.pos.x.is_finite()
                        || !v.pos.y.is_finite()
                        || !v.uv.x.is_finite()
                        || !v.uv.y.is_finite()
                })
            {
                return Err("mesh bounds".into());
            }
        }
        self.textures = staged;
        Ok(())
    }
    pub fn freed(&mut self, frame: &Frame) {
        for id in &frame.output.textures_delta.free {
            self.textures.remove(id);
        }
    }
}

pub(super) fn pixels(
    dir: &Path,
    generation: usize,
    frame: &Frame,
    rgba: &[u8],
    bottom_up: bool,
) -> Result<Value> {
    if rgba.len() != READBACK as usize {
        return Err("readback length".into());
    }
    let mut canonical = vec![0_u8; READBACK as usize];
    for y in 0..HEIGHT as usize {
        let source = if bottom_up {
            HEIGHT as usize - 1 - y
        } else {
            y
        };
        canonical[y * 5120..(y + 1) * 5120]
            .copy_from_slice(&rgba[source * 5120..(source + 1) * 5120]);
    }
    let stem = format!("g{generation}-f{:02}", frame.index);
    std::fs::write(dir.join(format!("{stem}.rgba")), &canonical).map_err(|e| e.to_string())?;
    let cyan = canonical
        .chunks_exact(4)
        .filter(|p| p[0] < 70 && p[1] > 140 && p[2] > 140)
        .count();
    let magenta = canonical
        .chunks_exact(4)
        .filter(|p| p[0] > 140 && p[1] < 90 && p[2] > 100)
        .count();
    let white = canonical
        .chunks_exact(4)
        .filter(|p| p[0] > 180 && p[1] > 180 && p[2] > 180)
        .count();
    let at = |x: usize, y: usize| -> [u8; 4] {
        canonical[(y * 1280 + x) * 4..(y * 1280 + x) * 4 + 4]
            .try_into()
            .unwrap()
    };
    let center = at(1164, 164);
    let corner = at(1110, 110);
    let sample = frame.receipt["sample"].as_array().unwrap();
    let sx = sample[0].as_f64().unwrap() as usize;
    let sy = sample[1].as_f64().unwrap() as usize;
    let mut sample_hits = 0;
    for y in sy.saturating_sub(4)..=(sy + 4).min(959) {
        for x in sx.saturating_sub(4)..=(sx + 4).min(1279) {
            let p = at(x, y);
            if p[0] < 70 && p[1] > 140 && p[2] > 140 {
                sample_hits += 1;
            }
        }
    }
    let red = |p: [u8; 4]| p[0] > 180 && p[1] < 70 && p[2] < 70;
    let green = |p: [u8; 4]| p[0] < 70 && p[1] > 180 && p[2] < 70;
    let texture_ok = match frame.index {
        6 | 10 | 11 => red(center),
        7 => green(center) && red(corner),
        8 => green(center) && green(corner),
        _ => true,
    };
    let pass = white > 20
        && (frame.index < 2 || (cyan > 100 && sample_hits > 0))
        && (frame.index < 3 || magenta > 100)
        && (frame.index != 0 || cyan == 0)
        && texture_ok;
    let mut receipt = frame.receipt.clone();
    receipt["pixels"] = json!({"cyan":cyan,"magenta":magenta,"white":white,"sample_hits":sample_hits,"texture_center":center,"texture_corner":corner,"pass":pass,"origin":"top-left RGBA8","bytes":canonical.len()});
    write(&dir.join(format!("{stem}.json")), &receipt);
    if !pass {
        return Err(format!("actual pixel check failed: {stem} {receipt}"));
    }
    Ok(receipt)
}
