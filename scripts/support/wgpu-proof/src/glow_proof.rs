//! Execute the same fixture against the unchanged Custom Glow Device.
use crate::{
    Result, contained_failure,
    device::Device,
    fixtures::{self, Bounds, Fixture},
    readonly_v2::{
        completion::{Completion, WaitPurpose, WaitRequest},
        frame::ContextGeneration,
        texture_owner::TextureKey,
    },
    write,
};
use serde_json::json;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

struct App {
    dir: PathBuf,
    ran: bool,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, events: &ActiveEventLoop) {
        if self.ran {
            return;
        }
        self.ran = true;
        let mut device = Device::new(events).unwrap();
        if let Err(error) = exercise(&mut device, &self.dir) {
            contained_failure(&self.dir, error);
        }
        events.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}
fn exercise(device: &mut Device, dir: &Path) -> Result<()> {
    write(
        &dir.join("adapter.json"),
        &serde_json::to_value(device.initialization()).unwrap(),
    );
    for number in 0..2 {
        let generation = ContextGeneration::allocate().map_err(|e| format!("{e:?}"))?;
        let mut fixture = Fixture::new();
        let mut bounds = Bounds::default();
        for index in 0..fixtures::FRAMES {
            let frame = fixture.frame(index);
            bounds.validate(&frame)?;
            for (id, delta) in fixtures::sorted_updates(&frame.output) {
                device.upload(TextureKey { generation, id }, delta)?;
            }
            device.draw(generation, &frame.meshes, 1.0)?;
            let start = Instant::now();
            let mut completed = false;
            for _ in 0..20 {
                let completion = device.wait(WaitRequest {
                    generation,
                    purpose: WaitPurpose::Frame,
                    timeout: Duration::from_millis(5),
                });
                if completion == Completion::Quiesced {
                    completed = true;
                    break;
                }
                if completion != Completion::Timeout || start.elapsed() > Duration::from_millis(100)
                {
                    break;
                }
            }
            if !completed {
                return Err("Glow completion unresolved".into());
            }
            device
                .release_frame(generation)
                .map_err(|e| format!("{e:?}"))?;
            fixtures::pixels(dir, number, &frame, device.readback()?, true)?;
            for id in &frame.output.textures_delta.free {
                device
                    .free(TextureKey {
                        generation,
                        id: *id,
                    })
                    .map_err(|e| format!("{e:?}"))?;
            }
            bounds.freed(&frame);
        }
        for id in bounds.textures.keys() {
            device
                .free(TextureKey {
                    generation,
                    id: *id,
                })
                .map_err(|e| format!("{e:?}"))?;
        }
        device
            .release_generation(generation)
            .map_err(|e| format!("{e:?}"))?;
    }
    let ledger = format!("{:?}", device.ledger());
    let counters = format!("{:?}", device.counters);
    device.healthy_close()?;
    write(
        &dir.join("result.json"),
        &json!({"frames":24,"generations":2,"ledger":ledger,"counters":counters,"healthy_close":true,"final_wgl_destruction":"LIMITED"}),
    );
    Ok(())
}
pub(super) fn run(dir: &Path) {
    let events = EventLoop::new().unwrap();
    events
        .run_app(&mut App {
            dir: dir.into(),
            ran: false,
        })
        .unwrap();
}
