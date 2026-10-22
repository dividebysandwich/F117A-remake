//! `SM89_PROFILE`: a performance profile written to the log every few seconds, for measuring
//! on hardware without a profiler (the Steam Frame).
//!
//! - Frames: average, 99th percentile and worst frame time.
//! - GPU: each 3D view's whole frame (`view`) and the render passes in it that record
//!   timestamps (Bevy's render diagnostics), summed over the views (both eyes in VR), with the
//!   CPU time spent encoding them and, where the GPU has pipeline statistics, the vertex and
//!   fragment shader invocations. Not on the Steam Frame: Turnip writes the timestamps without
//!   waiting for the work before them, so they read near zero. There, SteamVR's own
//!   measurement is reliable: `ApplicationTime CPU / GPU` in
//!   `~/.local/share/Steam/logs/vrcompositor.txt`, logged when the game exits.
//! - CPU: every system, schedule and render node (Bevy's trace spans, timed by a tracing
//!   layer). Spans nest (a schedule contains its systems) and systems on worker threads run in
//!   parallel, so these don't add up to the frame time.
//!
//! The value is a comma separated list: `1` (or `on`) profiles everything every 10 seconds,
//! `cpu` leaves out the GPU timestamps (a tiled GPU may render passes differently with them),
//! `<n>s` reports every n seconds. E.g. `SM89_PROFILE=1`, `SM89_PROFILE=cpu,30s`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::Duration;

use bevy::platform::time::Instant;

use bevy::diagnostic::DiagnosticsStore;
use bevy::log::BoxedLayer;
use bevy::log::tracing::Subscriber;
use bevy::log::tracing::field::{Field, Visit};
use bevy::log::tracing::span::{Attributes, Id};
use bevy::log::tracing_subscriber::layer::{Context, Layer};
use bevy::log::tracing_subscriber::registry::LookupSpan;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct ProfileConfig {
    /// Seconds between reports.
    pub interval: f32,
    /// GPU timestamps and pipeline statistics.
    pub gpu: bool,
}

/// The profile asked for by `SM89_PROFILE` (None: not profiling).
pub fn config() -> Option<ProfileConfig> {
    static CONFIG: OnceLock<Option<ProfileConfig>> = OnceLock::new();
    *CONFIG.get_or_init(|| {
        let v = std::env::var("SM89_PROFILE").ok()?;
        let v = v.trim();
        if v.is_empty() || v == "0" || v.eq_ignore_ascii_case("off") {
            return None;
        }
        let mut c = ProfileConfig { interval: 10.0, gpu: true };
        for t in v.split(',').map(str::trim) {
            if t.eq_ignore_ascii_case("cpu") {
                c.gpu = false;
            } else if let Some(s) = t.strip_suffix('s').and_then(|s| s.parse::<f32>().ok())
                && s > 0.0
            {
                c.interval = s;
            }
        }
        Some(c)
    })
}

/// Extra log filter: Bevy's per-system trace spans are switched off unless profiling (they
/// cost a little on every system run).
pub fn log_filter() -> &'static str {
    if config().is_some() { "" } else { "bevy_ecs=warn" }
}

/// `LogPlugin::custom_layer`: the span timer when profiling.
pub fn log_layer(_app: &mut App) -> Option<BoxedLayer> {
    config().map(|_| Box::new(SpanTimer) as BoxedLayer)
}

pub struct ProfilePlugin(pub ProfileConfig);

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        let c = self.0;
        info!(
            "profiling: report every {} s, GPU timestamps {}",
            c.interval,
            if c.gpu { "on" } else { "off" }
        );
        if c.gpu {
            use bevy::core_pipeline::{Core3d, Core3dSystems, upscaling::upscaling};
            app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
            // Each 3D view's whole frame, timed between passes.
            if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
                render.add_systems(
                    Core3d,
                    (
                        view_span_begin.before(Core3dSystems::Prepass),
                        view_span_end.after(upscaling),
                    ),
                );
            }
        }
        app.insert_resource(Report::new(c)).add_systems(Last, report);
    }
}

// Exclusive systems: they run on the thread running the view's schedule, and the recorder
// closes a span on the thread that opened it. Their command buffers are queued in order with
// the other systems' (which are applied before an exclusive system runs).
fn view_span_begin(world: &mut World) {
    view_span(world, true);
}

fn view_span_end(world: &mut World) {
    view_span(world, false);
}

fn view_span(world: &mut World, begin: bool) {
    use bevy::render::diagnostic::{DiagnosticsRecorder, RecordDiagnostics};
    use bevy::render::renderer::{PendingCommandBuffers, RenderDevice};
    let (Some(d), Some(device)) =
        (world.get_resource::<DiagnosticsRecorder>(), world.get_resource::<RenderDevice>())
    else {
        return;
    };
    let mut encoder = device.create_command_encoder(&Default::default());
    if begin {
        d.begin_time_span(&mut encoder, "view".into());
    } else {
        d.end_time_span(&mut encoder);
    }
    let buffer = encoder.finish();
    world.resource_mut::<PendingCommandBuffers>().push([buffer]);
}

// --- CPU: span timing --------------------------------------------------------------------------

#[derive(Default)]
struct CpuStat {
    total: Duration,
    calls: u32,
    max: Duration,
}

/// Time spent in each span label since the last report.
static CPU: LazyLock<Mutex<HashMap<Arc<str>, CpuStat>>> = LazyLock::new(Default::default);

/// Per span: its report label and when it was entered.
struct SpanTiming {
    label: Arc<str>,
    start: Option<Instant>,
}

/// The `name` field (systems, schedules) and message (shadow passes) of a span.
#[derive(Default)]
struct SpanFields {
    name: Option<String>,
    message: Option<String>,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "name" => self.name = Some(value.to_string()),
            "message" => self.message = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "name" => self.name = Some(format!("{value:?}")),
            "message" => self.message = Some(format!("{value:?}")),
            _ => {}
        }
    }
}

struct SpanTimer;

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for SpanTimer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut f = SpanFields::default();
        attrs.record(&mut f);
        let short = |n: &str| n.replace("stealth_mission_89::", "");
        let label = match (attrs.metadata().name(), f.name) {
            ("system", Some(n)) => short(&n),
            ("system_commands", Some(n)) => format!("commands {}", short(&n)),
            (s, Some(n)) => format!("{s} {n}"),
            ("", None) => f.message.unwrap_or_default(),
            (s, None) => s.to_string(),
        };
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanTiming { label: label.into(), start: None });
        }
    }

    fn on_enter(&self, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id)
            && let Some(t) = span.extensions_mut().get_mut::<SpanTiming>()
        {
            t.start = Some(Instant::now());
        }
    }

    fn on_exit(&self, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        let Some(t) = ext.get_mut::<SpanTiming>() else { return };
        let Some(start) = t.start.take() else { return };
        let dt = start.elapsed();
        let label = t.label.clone();
        drop(ext);
        let mut cpu = CPU.lock().unwrap_or_else(|e| e.into_inner());
        let s = cpu.entry(label).or_default();
        s.total += dt;
        s.calls += 1;
        s.max = s.max.max(dt);
    }
}

// --- Report ------------------------------------------------------------------------------------

/// One render pass's diagnostics, summed per frame.
#[derive(Default)]
struct PassStat {
    /// Field (elapsed_gpu, elapsed_cpu, fragment_shader_invocations, ...) -> (sum, worst frame).
    fields: HashMap<String, (f64, f64)>,
    /// Measurements per frame of the GPU time (views drawing this pass).
    views: u32,
}

#[derive(Resource)]
struct Report {
    config: ProfileConfig,
    since: Instant,
    last_frame: Option<Instant>,
    /// Frame times (ms) since the last report.
    frames: Vec<f32>,
    passes: HashMap<String, PassStat>,
    /// Newest measurement taken from each diagnostic.
    seen: HashMap<String, Instant>,
}

impl Report {
    fn new(config: ProfileConfig) -> Self {
        Self {
            config,
            since: Instant::now(),
            last_frame: None,
            frames: Vec::new(),
            passes: HashMap::new(),
            seen: HashMap::new(),
        }
    }

    /// This frame's new render diagnostics (sync_diagnostics adds one batch per rendered frame).
    fn gather_gpu(&mut self, store: &DiagnosticsStore) {
        let mut frame: HashMap<(String, String), (f64, u32)> = HashMap::new();
        for d in store.iter() {
            let path = d.path().as_str();
            let Some((pass, field)) = path.strip_prefix("render/").and_then(|p| p.rsplit_once('/'))
            else {
                continue;
            };
            let last = self.seen.get(path).copied();
            let mut newest = last;
            for m in d.measurements().filter(|m| last.is_none_or(|t| m.time > t)) {
                let e = frame.entry((pass.to_string(), field.to_string())).or_default();
                e.0 += m.value;
                e.1 += 1;
                newest = newest.max(Some(m.time));
            }
            if let Some(t) = newest {
                self.seen.insert(path.to_string(), t);
            }
        }
        for ((pass, field), (v, n)) in frame {
            let p = self.passes.entry(pass).or_default();
            if field == "elapsed_gpu" {
                p.views = p.views.max(n);
            }
            let f = p.fields.entry(field).or_default();
            f.0 += v;
            f.1 = f.1.max(v);
        }
    }
}

fn report(
    mut r: ResMut<Report>,
    store: Option<Res<DiagnosticsStore>>,
    cameras: Query<(&Camera, Option<&Msaa>)>,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
    entities: Query<Entity>,
) {
    let now = Instant::now();
    if let Some(l) = r.last_frame {
        let ms = (now - l).as_secs_f32() * 1000.0;
        r.frames.push(ms);
    } else {
        // Start up and loading are not part of the first report.
        CPU.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    r.last_frame = Some(now);
    if let Some(store) = &store {
        r.gather_gpu(store);
    }
    let secs = (now - r.since).as_secs_f32();
    if secs < r.config.interval || r.frames.is_empty() {
        return;
    }

    use std::fmt::Write;
    let mut out = String::new();
    let n = r.frames.len();
    let per_frame = 1.0 / n as f64;
    let mut sorted = r.frames.clone();
    sorted.sort_by(f32::total_cmp);
    let avg = sorted.iter().sum::<f32>() / n as f32;
    let p99 = sorted[((n - 1) as f32 * 0.99).round() as usize];
    let _ = writeln!(
        out,
        "profile: {secs:.1} s, {n} frames: {avg:.2} ms avg ({:.1} fps), p99 {p99:.2} ms, worst {:.2} ms",
        1000.0 / avg,
        sorted[n - 1]
    );
    for (cam, msaa) in cameras.iter().filter(|(c, _)| c.is_active) {
        if let Some(size) = cam.physical_viewport_size() {
            let _ = writeln!(
                out,
                "  view {}x{} order {} msaa {}",
                size.x,
                size.y,
                cam.order,
                msaa.map_or(1, |m| m.samples())
            );
        }
    }
    let visible = meshes.iter().filter(|v| v.get()).count();
    let _ = writeln!(
        out,
        "  entities {}, meshes {} ({visible} visible)",
        entities.iter().count(),
        meshes.iter().count()
    );

    if !r.passes.is_empty() {
        let _ = writeln!(
            out,
            "  GPU per frame, summed over views (pass: gpu ms [worst] / encode cpu ms / vertex, fragment invocations):"
        );
        let mut passes: Vec<_> = r.passes.iter().collect();
        let gpu = |p: &PassStat| p.fields.get("elapsed_gpu").map_or(0.0, |f| f.0);
        passes.sort_by(|a, b| gpu(b.1).total_cmp(&gpu(a.1)).then(a.0.cmp(b.0)));
        let total: f64 = passes.iter().filter(|(k, _)| !k.contains('/')).map(|(_, p)| gpu(p)).sum();
        let _ = writeln!(out, "    total (top-level passes) {:.2} ms", total * per_frame);
        for (name, p) in passes {
            let f = |k: &str| p.fields.get(k).copied();
            let mut line = format!("    {name}:");
            if let Some((sum, worst)) = f("elapsed_gpu") {
                let _ = write!(line, " gpu {:.2} ms [{worst:.2}] x{}", sum * per_frame, p.views);
            }
            if let Some((sum, _)) = f("elapsed_cpu") {
                let _ = write!(line, " / cpu {:.2} ms", sum * per_frame);
            }
            // (Turnip returns garbage pipeline statistics for some passes.)
            if let (Some((v, _)), Some((fr, _))) =
                (f("vertex_shader_invocations"), f("fragment_shader_invocations"))
                && v * per_frame < 1e10
                && fr * per_frame < 1e10
            {
                let _ = write!(
                    line,
                    " / {:.2} M vs, {:.2} M fs",
                    v * per_frame / 1e6,
                    fr * per_frame / 1e6
                );
            }
            let _ = writeln!(out, "{line}");
        }
    } else if r.config.gpu {
        let _ =
            writeln!(out, "  GPU: no timestamps (the device lacks TIMESTAMP_QUERY_INSIDE_PASSES?)");
    }

    let mut cpu: Vec<_> =
        std::mem::take(&mut *CPU.lock().unwrap_or_else(|e| e.into_inner())).into_iter().collect();
    cpu.sort_by_key(|(_, s)| std::cmp::Reverse(s.total));
    let _ = writeln!(out, "  CPU per frame, top spans (ms avg / worst call / calls per frame):");
    for (label, s) in
        cpu.iter().take(100).filter(|(_, s)| s.total.as_secs_f64() * 1000.0 * per_frame >= 0.02)
    {
        let mut l: String = label.chars().take(110).collect();
        if label.chars().count() > 110 {
            l.push('~');
        }
        let _ = writeln!(
            out,
            "    {:7.3} {:7.3} {:5.1}  {l}",
            s.total.as_secs_f64() * 1000.0 * per_frame,
            s.max.as_secs_f64() * 1000.0,
            s.calls as f64 * per_frame
        );
    }
    // Systems (and their commands) by crate, the game's by module: what each plugin costs,
    // small systems included.
    let mut crates: HashMap<&str, (Duration, u32)> = HashMap::new();
    for (label, s) in &cpu {
        let path = label.strip_prefix("commands ").unwrap_or(label);
        if let Some((first, _)) = path.split_once("::")
            && !first.contains(' ')
        {
            let e = crates.entry(first).or_default();
            e.0 += s.total;
            e.1 += 1;
        }
    }
    let mut crates: Vec<_> = crates.into_iter().collect();
    crates.sort_by_key(|(_, (t, _))| std::cmp::Reverse(*t));
    let _ = writeln!(out, "  CPU per frame by crate / game module (ms, systems):");
    for (name, (t, n)) in crates {
        let _ = writeln!(out, "    {:7.3} {n:4}  {name}", t.as_secs_f64() * 1000.0 * per_frame);
    }
    info!("{}", out.trim_end());

    r.since = now;
    r.frames.clear();
    r.passes.clear();
}
