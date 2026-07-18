//! Opt-in native renderer regression. Input is synthetic egui input, never OS input.
use super::*;

type Baseline = (u64, u64, usize);

pub(super) struct Probe {
    mode: String,
    started: Instant,
    baseline: Option<HashMap<ViewportId, Baseline>>,
    next_input: Instant,
    dragging: bool,
    pub report: Option<serde_json::Value>,
}

impl Probe {
    pub fn new() -> Result<Option<Self>, String> {
        let Some(mode) = std::env::var_os("MMB_RENDER_TEST") else {
            return Ok(None);
        };
        let mode = mode.to_string_lossy().into_owned();
        if !std::env::args().any(|a| a == "--demo")
            || !["hidden", "settings", "popup", "slider"].contains(&mode.as_str())
        {
            return Err(
                "Renderer probes require --demo and a supported MMB_RENDER_TEST mode.".into(),
            );
        }
        let started = Instant::now();
        Ok(Some(Self {
            mode,
            started,
            baseline: None,
            next_input: started,
            dragging: false,
            report: None,
        }))
    }

    pub fn next(&self) -> Instant {
        if self.baseline.is_none() {
            self.started + Duration::from_secs(3)
        } else if self.mode == "slider" {
            self.next_input
        } else {
            self.started + Duration::from_secs(8)
        }
    }

    pub fn tick(&mut self, host: &mut Host, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.started);
        if elapsed < Duration::from_secs(3) {
            return;
        }
        if self.baseline.is_none() {
            self.baseline = Some(
                host.views
                    .iter()
                    .map(|(id, v)| {
                        (
                            *id,
                            (
                                v.stats.rasterized,
                                v.stats.ui_passes,
                                v.stats.times_ms.len(),
                            ),
                        )
                    })
                    .collect(),
            );
        }
        if elapsed >= Duration::from_secs(8) {
            let views: Vec<_> = host.views.iter().map(|(id,v)| {
                let (raster,ui,times) = self.baseline.as_ref().unwrap().get(id).copied().unwrap_or_default();
                let mut ms = v.stats.times_ms[times..].to_vec();
                ms.sort_by(f64::total_cmp);
                let n = ms.len();
                serde_json::json!({"title":v.window.title(), "visible":v.window.is_visible(),
                    "rasterized":v.stats.rasterized-raster, "ui_passes":v.stats.ui_passes-ui,
                    "hidden_paints":v.stats.hidden_paints,
                    "frame_median_ms":ms.get(n/2), "frame_p95_ms":ms.get(n.saturating_sub(1)*95/100),
                    "frame_max_ms":ms.last()})
            }).collect();
            let rasterized: u64 = views
                .iter()
                .map(|v| v["rasterized"].as_u64().unwrap())
                .sum();
            let hidden_paints: u64 = views
                .iter()
                .map(|v| v["hidden_paints"].as_u64().unwrap())
                .sum();
            let visible = views.iter().any(|v| v["visible"] == true);
            let passed = hidden_paints == 0
                && if self.mode == "slider" {
                    visible && rasterized >= 10
                } else {
                    rasterized == 0 && (visible == (self.mode != "hidden"))
                };
            self.report = Some(serde_json::json!({"mode":self.mode,"passed":passed,
                "warmup_seconds":3,"sample_seconds":5,"views":views,
                "input":"controlled egui input; simulated monitors; native pointer ignored; no OS input injection"}));
            if !passed {
                host.error = Some(format!("Renderer probe failed: {:?}", self.report));
            }
            event_loop.exit();
            return;
        }
        if self.mode == "slider" && now >= self.next_input {
            let slider = host.ctx.data(|data| {
                data.get_temp::<(ViewportId, egui::Rect, u8)>(egui::Id::unique(
                    "render-probe-slider",
                ))
            });
            if let Some((id, rect, _)) = slider {
                let fraction = 0.2 + 0.6 * ((elapsed.as_secs_f32() - 3.0) / 5.0).clamp(0.0, 1.0);
                let pos = egui::pos2(egui::lerp(rect.x_range(), fraction), rect.center().y);
                let view = host.views.get_mut(&id).unwrap();
                view.input
                    .egui_input_mut()
                    .events
                    .push(egui::Event::PointerMoved(pos));
                if !self.dragging {
                    view.input
                        .egui_input_mut()
                        .events
                        .push(egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Default::default(),
                        });
                    self.dragging = true;
                }
                host.schedule(id, now);
            }
            self.next_input = now + Duration::from_millis(16);
        }
    }
}
