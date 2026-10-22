//! Hull raster, then a fixed-step ray march (Bevy `FogVolume` style).
//!
//! Drawn for every view with a [`CloudCamera`] (both eyes in VR) between the opaque and the
//! transparent pass, into the view's (multisampled) colour target: opaque geometry clips the
//! march through the depth buffer, transparent geometry (glass, smoke, lights) is drawn over the
//! clouds afterwards. The march goes into a texture per view (smaller with a resolution scale
//! above 1) with the distance at which the clouds turn opaque; it is upsampled over the view
//! and that distance written into the view's depth, so transparent geometry behind the clouds
//! is hidden.

use std::collections::HashMap;

use bevy::{
    asset::{embedded_asset, load_embedded_asset},
    camera::Camera,
    core_pipeline::{
        Core3dSystems, FullscreenShader,
        core_3d::CORE_3D_DEPTH_FORMAT,
        core_3d::{main_opaque_pass_3d, main_transparent_pass_3d},
        schedule::Core3d,
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{
            binding_types::{
                sampler, storage_buffer_read_only, texture_2d, texture_depth_2d,
                texture_depth_2d_multisampled, uniform_buffer,
            },
            *,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::{CachedTexture, TextureCache},
        view::{
            ExtractedView, Msaa, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
    shader::ShaderDefVal,
};

use crate::{
    lod::{CloudRenderSettings, LodPolicy, lod_bands},
    volume::{MAX_SPHERES, MAX_VOLUMES, WaveletVolume},
};

/// Volume instance. LOD overwrites `j_max` / `h_max` each frame.
#[derive(Component, Clone)]
pub struct CloudVolume {
    pub volume: Handle<WaveletVolume>,
    pub j_max: u32,
    pub h_max: u32,
    /// Density scale; lowering it thins and shrinks the cloud (0: not drawn).
    pub fill: f32,
    /// 0: wisps of the ridges alone; 1: a solid core from the spheres, its edges eroded by
    /// the ridges.
    pub core: f32,
    /// 0..1: how much darker than the shade ramp the cloud is drawn (storm cells).
    pub darken: f32,
}

impl CloudVolume {
    pub fn new(volume: Handle<WaveletVolume>) -> Self {
        Self { volume, j_max: 3, h_max: 4, fill: 1.0, core: 0.0, darken: 0.0 }
    }

    pub fn with_fill(self, fill: f32) -> Self {
        Self { fill: fill.max(0.0), ..self }
    }

    pub fn with_core(self, core: f32) -> Self {
        Self { core: core.clamp(0.0, 1.0), ..self }
    }

    pub fn with_darken(self, darken: f32) -> Self {
        Self { darken: darken.clamp(0.0, 1.0), ..self }
    }
}

/// A 3D camera that draws the clouds. The first one also picks the volumes' LOD and whether
/// the viewpoint is inside a volume.
#[derive(Component, Clone, Copy, Default, ExtractComponent)]
pub struct CloudCamera;

/// Light and colour of the clouds (set by the application, e.g. by time of day).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudShading {
    /// Direction towards the sun.
    pub sun_dir: Vec3,
    /// Shade ramp, linear RGB: entry 0 is sunlit cloud (and the colour distant clouds haze
    /// into), entry 15 the darkest shadow.
    pub ramp: [Vec4; 16],
    /// Distance (world units) at which clouds have hazed all the way down the ramp to entry
    /// 0; 0 for no haze.
    pub haze_distance: f32,
    /// Light added to every lit sample (a lightning flash), linear RGB.
    pub flash: Vec3,
    /// A line light (a lightning channel): one end and intensity, the other end and reach
    /// (world units), and its linear colour.
    pub line_a: Vec4,
    pub line_b: Vec4,
    pub line_color: Vec3,
    /// Fog over the clouds (inside another cloud): colour, amount 0..1.
    pub fog: Vec4,
}

impl Default for CloudShading {
    fn default() -> Self {
        Self {
            sun_dir: Vec3::new(0.35, 1.0, 0.2).normalize(),
            ramp: std::array::from_fn(|i| Vec3::splat(0.9 - 0.045 * i as f32).extend(1.0)),
            haze_distance: 0.0,
            flash: Vec3::ZERO,
            line_a: Vec4::ZERO,
            line_b: Vec4::ZERO,
            line_color: Vec3::ONE,
            fog: Vec4::ZERO,
        }
    }
}

/// Per frame constants shared by all views.
#[derive(Clone, Copy, ShaderType, Default)]
pub struct CloudGlobals {
    /// Direction to the sun, haze distance.
    pub sun: Vec4,
    /// Max march span, sigma_t, phase g, LOD false colour.
    pub params0: Vec4,
    /// j cap, keep alpha, interior span, frame counter.
    pub params1: Vec4,
    /// Resolution scale.
    pub params2: Vec4,
    pub flash: Vec4,
    pub line_a: Vec4,
    pub line_b: Vec4,
    pub line_color: Vec4,
    pub fog: Vec4,
    pub ramp: [Vec4; 16],
}

#[derive(Clone, Copy, ShaderType, Default)]
pub struct GpuVolume {
    pub aabb_min: Vec3,
    pub j_max: f32,
    pub aabb_max: Vec3,
    pub h_max: f32,
    pub misc: UVec4,
    pub fill: Vec4,
}

/// Everything the draw needs from the main world: volumes sorted far to near within each
/// batch, their spheres, and the batches (exterior LOD 0–3, then interior LOD 0–3:
/// `(first, count)`).
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct CloudFrame {
    pub globals: CloudGlobals,
    pub volumes: Vec<GpuVolume>,
    pub spheres: Vec<Vec4>,
    pub batches: [UVec2; 8],
    pub steps: [u32; 4],
    pub dither: bool,
    pub sun_shadow: bool,
    pub resolution_scale: u32,
}

#[derive(Resource)]
struct CloudBuffers {
    globals: UniformBuffer<CloudGlobals>,
    volumes: StorageBuffer<Vec<GpuVolume>>,
    spheres: StorageBuffer<Vec<Vec4>>,
}

pub(crate) fn build_render(app: &mut App) {
    embedded_asset!(app, "shaders/wavelet_cloud.wgsl");
    embedded_asset!(app, "shaders/cloud_composite.wgsl");
    app.init_resource::<CloudShading>()
        .init_resource::<CloudFrame>()
        .add_plugins((
            ExtractComponentPlugin::<CloudCamera>::default(),
            ExtractResourcePlugin::<CloudFrame>::default(),
        ))
        .add_systems(
            PostUpdate,
            (update_volume_lod, upload_cloud_frame)
                .chain()
                .in_set(crate::CloudSystems)
                .after(bevy::transform::TransformSystems::Propagate),
        );

    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.add_systems(RenderStartup, init_cloud_pipeline);
    render_app.add_systems(
        Render,
        (
            (prepare_cloud_buffers, prepare_cloud_targets).in_set(RenderSystems::PrepareResources),
            queue_cloud_pipelines.in_set(RenderSystems::Prepare),
        ),
    );
    render_app.add_systems(
        Core3d,
        draw_cloud_hulls
            .after(main_opaque_pass_3d)
            .before(main_transparent_pass_3d)
            .in_set(Core3dSystems::MainPass),
    );
}

fn aabb_contains(min: Vec3, max: Vec3, p: Vec3) -> bool {
    p.cmpge(min).all() && p.cmple(max).all()
}

fn world_aabb(tf: &GlobalTransform, aabb_min: Vec3, aabb_max: Vec3) -> (Vec3, Vec3) {
    let mut wmin = Vec3::splat(f32::MAX);
    let mut wmax = Vec3::splat(f32::MIN);
    for x in [aabb_min.x, aabb_max.x] {
        for y in [aabb_min.y, aabb_max.y] {
            for z in [aabb_min.z, aabb_max.z] {
                let p = tf.transform_point(Vec3::new(x, y, z));
                wmin = wmin.min(p);
                wmax = wmax.max(p);
            }
        }
    }
    (wmin, wmax)
}

/// The camera that picks LODs: the first cloud camera.
type LodCameras<'w, 's> = Query<
    'w,
    's,
    (&'static GlobalTransform, &'static Projection, &'static Camera),
    With<CloudCamera>,
>;

pub(crate) fn update_volume_lod(
    cameras: LodCameras,
    mut volumes: Query<(&mut CloudVolume, &GlobalTransform)>,
    assets: Res<Assets<WaveletVolume>>,
    policy: Res<LodPolicy>,
    settings: Res<CloudRenderSettings>,
) {
    let Some((cam_tf, projection, camera)) = cameras.iter().next() else {
        return;
    };
    let cam_pos = cam_tf.translation();
    let fov = match projection {
        Projection::Perspective(p) => p.fov,
        // XR eyes: about the headset's field of view.
        _ => 1.7,
    };
    let height = camera.logical_viewport_size().map(|s| s.y).unwrap_or(720.0);
    for (mut volume, tf) in &mut volumes {
        let Some(asset) = assets.get(&volume.volume) else {
            continue;
        };
        let (aabb_min, aabb_max) = world_aabb(tf, asset.aabb_min, asset.aabb_max);
        let center = (aabb_min + aabb_max) * 0.5;
        let extent = (aabb_max - aabb_min).length();
        let d = cam_pos.distance(center);
        let (j_max, h_max) = lod_bands(
            d,
            extent,
            fov,
            height,
            &policy,
            settings.j_cap.min(asset.j_cap),
            settings.h_cap.min(asset.h_cap),
        );
        let interior = aabb_contains(aabb_min, aabb_max, cam_pos);
        volume.j_max = if interior { j_max.min(1) } else { j_max };
        volume.h_max = if interior { h_max.min(2) } else { h_max };
    }
}

pub(crate) fn upload_cloud_frame(
    cameras: LodCameras,
    volumes: Query<(&CloudVolume, &GlobalTransform, &InheritedVisibility)>,
    assets: Res<Assets<WaveletVolume>>,
    settings: Res<CloudRenderSettings>,
    shading: Res<CloudShading>,
    mut frame: ResMut<CloudFrame>,
    mut counter: Local<u32>,
) {
    let frame = frame.as_mut();
    frame.volumes.clear();
    frame.spheres.clear();
    frame.batches = [UVec2::ZERO; 8];
    let Some((cam_tf, _, _)) = cameras.iter().next() else {
        return;
    };
    let cam = cam_tf.translation();
    *counter = counter.wrapping_add(1);
    let s = &shading;
    frame.globals = CloudGlobals {
        sun: s.sun_dir.normalize_or(Vec3::Y).extend(s.haze_distance),
        params0: Vec4::new(
            settings.max_march_span,
            settings.sigma_t,
            settings.phase_g,
            if settings.lod_false_color { 1.0 } else { 0.0 },
        ),
        params1: Vec4::new(
            settings.j_cap as f32,
            settings.keep_alpha,
            settings.interior_span,
            (*counter % 4096) as f32,
        ),
        params2: Vec4::new(settings.resolution_scale.max(1) as f32, 0.0, 0.0, 0.0),
        flash: s.flash.extend(0.0),
        line_a: s.line_a,
        line_b: s.line_b,
        line_color: s.line_color.extend(0.0),
        fog: s.fog,
        ramp: s.ramp,
    };
    frame.steps = std::array::from_fn(|j| settings.steps_for_lod(j as u32));
    frame.dither = settings.dither;
    frame.sun_shadow = settings.sun_shadow;
    frame.resolution_scale = settings.resolution_scale.max(1);

    struct PackedCloud {
        lod: u32,
        interior: u32,
        /// Squared distance to the camera, for drawing far to near.
        dist2: f32,
        gpu: GpuVolume,
        tf: GlobalTransform,
        volume: AssetId<WaveletVolume>,
    }

    let mut packed = Vec::new();
    for (volume, tf, visible) in &volumes {
        let Some(asset) = assets.get(&volume.volume) else {
            continue;
        };
        if !visible.get() || volume.fill <= 0.0 {
            continue;
        }
        let (aabb_min, aabb_max) = world_aabb(tf, asset.aabb_min, asset.aabb_max);
        let interior = u32::from(aabb_contains(aabb_min, aabb_max, cam));
        packed.push(PackedCloud {
            lod: volume.j_max.min(3),
            interior,
            dist2: cam.distance_squared(cam.clamp(aabb_min, aabb_max)),
            gpu: GpuVolume {
                aabb_min,
                j_max: volume.j_max as f32,
                aabb_max,
                h_max: volume.h_max as f32,
                misc: UVec4::new(0, 0, asset.seed, interior),
                fill: Vec4::new(volume.fill, volume.core, volume.darken, 0.0),
            },
            tf: *tf,
            volume: volume.volume.id(),
        });
    }
    // Nearest first when there are too many; then by batch, far to near within each.
    packed.sort_by(|a, b| a.dist2.total_cmp(&b.dist2));
    packed.truncate(MAX_VOLUMES);
    packed.sort_by(|a, b| {
        (a.interior, a.lod).cmp(&(b.interior, b.lod)).then(b.dist2.total_cmp(&a.dist2))
    });

    for cloud in &mut packed {
        let batch = (cloud.interior * 4 + cloud.lod) as usize;
        if frame.batches[batch].y == 0 {
            frame.batches[batch].x = frame.volumes.len() as u32;
        }
        frame.batches[batch].y += 1;
        let asset = assets.get(cloud.volume).expect("checked above");
        let offset = frame.spheres.len() as u32;
        let scale = cloud.tf.scale().y.abs().max(0.001);
        for sphere in &asset.spheres {
            if frame.spheres.len() >= MAX_SPHERES {
                break;
            }
            let center = cloud.tf.transform_point(sphere.center);
            frame.spheres.push(center.extend(sphere.radius * scale));
        }
        cloud.gpu.misc.x = offset;
        cloud.gpu.misc.y = frame.spheres.len() as u32 - offset;
        frame.volumes.push(cloud.gpu);
    }
}

/// Formats of the cloud textures: premultiplied colour and coverage, and the view depth (view
/// z, world units) at which a cloud turns opaque.
const CLOUD_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
const DISTANCE_FORMAT: TextureFormat = TextureFormat::R16Float;
/// Cloud distance where no cloud is opaque (the largest half float).
const NO_CLOUD: f32 = 65504.0;

/// Hull pipelines per (depth, march steps, dither, shadow, resolution), each one per LOD for
/// exterior and interior hulls.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct PipelineKey {
    depth_multisampled: bool,
    steps: [u32; 4],
    dither: bool,
    sun_shadow: bool,
    scale: u32,
}

/// Composite pipelines per (view format, samples): colour, then cloud depth.
type CompositeKey = (TextureFormat, u32);

#[derive(Resource)]
struct CloudPipelines {
    /// Single sampled and multisampled depth.
    layouts: [BindGroupLayoutDescriptor; 2],
    shader: Handle<Shader>,
    /// Exterior LOD 0–3, then interior LOD 0–3.
    ids: HashMap<PipelineKey, [CachedRenderPipelineId; 8]>,
    composite_layout: BindGroupLayoutDescriptor,
    composite_shader: Handle<Shader>,
    fullscreen: FullscreenShader,
    composites: HashMap<CompositeKey, [CachedRenderPipelineId; 2]>,
    /// Bilinear upsampling; nearest for the dithered look (chunky pixels).
    linear: Sampler,
    nearest: Sampler,
}

/// The view's pipelines: hulls, then the composite's colour and depth.
#[derive(Component)]
struct ViewCloudPipelines([CachedRenderPipelineId; 8], [CachedRenderPipelineId; 2]);

/// The view's cloud textures: colour and distance.
#[derive(Component)]
struct ViewCloudTarget(CachedTexture, CachedTexture);

/// Premultiplied colour and coverage over what is there.
const PREMULTIPLIED_OVER: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

/// The nearest of overlapping clouds' distances.
const NEAREST: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Min,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Min,
    },
};

fn lod_shader_defs(key: &PipelineKey, j_max: u32) -> Vec<ShaderDefVal> {
    let mut defs = Vec::new();
    if j_max >= 1 {
        defs.push("J1".into());
    }
    if j_max >= 2 {
        defs.push("J2".into());
    }
    if j_max >= 3 {
        defs.push("J3".into());
    }
    if key.depth_multisampled {
        defs.push("MULTISAMPLED".into());
    }
    if key.dither {
        defs.push("DITHER".into());
    }
    if key.sun_shadow {
        defs.push("SUN_SHADOW".into());
    }
    defs.push(ShaderDefVal::Int("MARCH_STEPS".into(), key.steps[j_max as usize] as i32));
    defs.push(ShaderDefVal::Int("RES_SCALE".into(), key.scale as i32));
    defs
}

fn hull_pipeline(
    pipelines: &CloudPipelines,
    pipeline_cache: &PipelineCache,
    key: &PipelineKey,
    interior: bool,
    j_max: u32,
) -> CachedRenderPipelineId {
    let defs = lod_shader_defs(key, j_max);
    let layout = pipelines.layouts[key.depth_multisampled as usize].clone();
    let shader = pipelines.shader.clone();
    let kind = if interior { "interior" } else { "exterior" };
    pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some(format!("malvar_ridge_{kind}_j{j_max}").into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: shader.clone(),
            shader_defs: defs.clone(),
            buffers: vec![],
            ..default()
        },
        primitive: PrimitiveState {
            topology: PrimitiveTopology::TriangleList,
            cull_mode: Some(if interior { Face::Front } else { Face::Back }),
            ..default()
        },
        fragment: Some(FragmentState {
            shader,
            shader_defs: defs,
            targets: vec![
                Some(ColorTargetState {
                    format: CLOUD_FORMAT,
                    blend: Some(PREMULTIPLIED_OVER),
                    write_mask: ColorWrites::ALL,
                }),
                Some(ColorTargetState {
                    format: DISTANCE_FORMAT,
                    blend: Some(NEAREST),
                    write_mask: ColorWrites::RED,
                }),
            ],
            ..default()
        }),
        ..default()
    })
}

/// Upsampling the clouds over the view (depth tested always, not written), then their depth
/// where they are opaque (tested, so nearer geometry keeps its own, colour not written).
fn composite_pipeline(
    pipelines: &CloudPipelines,
    pipeline_cache: &PipelineCache,
    (format, samples): CompositeKey,
    depth_write: bool,
) -> CachedRenderPipelineId {
    pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some(if depth_write { "cloud_depth" } else { "cloud_composite" }.into()),
        layout: vec![pipelines.composite_layout.clone()],
        vertex: pipelines.fullscreen.to_vertex_state(),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write),
            depth_compare: Some(if depth_write {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Always
            }),
            stencil: default(),
            bias: default(),
        }),
        multisample: MultisampleState { count: samples, ..default() },
        fragment: Some(FragmentState {
            shader: pipelines.composite_shader.clone(),
            entry_point: Some(if depth_write { "depth" } else { "fragment" }.into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: Some(PREMULTIPLIED_OVER),
                write_mask: if depth_write { ColorWrites::empty() } else { ColorWrites::ALL },
            })],
            ..default()
        }),
        ..default()
    })
}

fn init_cloud_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fullscreen: Res<FullscreenShader>,
    render_device: Res<RenderDevice>,
) {
    let layout = |label: &'static str, multisampled: bool| {
        BindGroupLayoutDescriptor::new(
            label,
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    if multisampled { texture_depth_2d_multisampled() } else { texture_depth_2d() },
                    uniform_buffer::<ViewUniform>(true),
                    uniform_buffer::<CloudGlobals>(false),
                    storage_buffer_read_only::<Vec<GpuVolume>>(false),
                    storage_buffer_read_only::<Vec<Vec4>>(false),
                ),
            ),
        )
    };
    commands.insert_resource(CloudPipelines {
        layouts: [
            layout("cloud_hull_bind_group_layout", false),
            layout("cloud_hull_bind_group_layout_msaa", true),
        ],
        shader: load_embedded_asset!(asset_server.as_ref(), "shaders/wavelet_cloud.wgsl"),
        ids: HashMap::new(),
        composite_layout: BindGroupLayoutDescriptor::new(
            "cloud_composite_bind_group_layout",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    uniform_buffer::<ViewUniform>(true),
                    uniform_buffer::<CloudGlobals>(false),
                    texture_2d(TextureSampleType::Float { filterable: false }),
                ),
            ),
        ),
        composite_shader: load_embedded_asset!(
            asset_server.as_ref(),
            "shaders/cloud_composite.wgsl"
        ),
        fullscreen: fullscreen.clone(),
        composites: HashMap::new(),
        linear: render_device.create_sampler(&SamplerDescriptor {
            label: Some("cloud_composite_linear"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        nearest: render_device.create_sampler(&SamplerDescriptor {
            label: Some("cloud_composite_nearest"),
            ..default()
        }),
    });
    commands.insert_resource(CloudBuffers {
        globals: UniformBuffer::default(),
        volumes: StorageBuffer::from(vec![GpuVolume::default()]),
        spheres: StorageBuffer::from(vec![Vec4::ZERO]),
    });
}

fn queue_cloud_pipelines(
    mut commands: Commands,
    views: Query<(Entity, &ViewTarget, &Msaa), With<CloudCamera>>,
    frame: Res<CloudFrame>,
    mut pipelines: ResMut<CloudPipelines>,
    pipeline_cache: Res<PipelineCache>,
) {
    for (entity, target, msaa) in &views {
        let key = PipelineKey {
            depth_multisampled: msaa.samples() > 1,
            steps: frame.steps,
            dither: frame.dither,
            sun_shadow: frame.sun_shadow,
            scale: frame.resolution_scale,
        };
        let ids = match pipelines.ids.get(&key) {
            Some(ids) => *ids,
            None => {
                let p = &*pipelines;
                let ids: [CachedRenderPipelineId; 8] = std::array::from_fn(|i| {
                    hull_pipeline(p, &pipeline_cache, &key, i >= 4, (i % 4) as u32)
                });
                pipelines.ids.insert(key, ids);
                ids
            }
        };
        let key = (target.main_texture_format(), msaa.samples());
        let composite = match pipelines.composites.get(&key) {
            Some(ids) => *ids,
            None => {
                let p = &*pipelines;
                let ids = [false, true].map(|d| composite_pipeline(p, &pipeline_cache, key, d));
                pipelines.composites.insert(key, ids);
                ids
            }
        };
        commands.entity(entity).insert(ViewCloudPipelines(ids, composite));
    }
}

/// The cloud textures of each view (a reduced resolution with a resolution scale above 1).
fn prepare_cloud_targets(
    mut commands: Commands,
    views: Query<(Entity, &ExtractedView), With<CloudCamera>>,
    frame: Res<CloudFrame>,
    mut cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
) {
    let scale = frame.resolution_scale.max(1);
    for (entity, view) in &views {
        let size = ((view.viewport.zw() + (scale - 1)) / scale).max(UVec2::ONE);
        let mut texture = |label, format| {
            cache.get(
                &render_device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d { width: size.x, height: size.y, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let target = ViewCloudTarget(
            texture("cloud_colour", CLOUD_FORMAT),
            texture("cloud_distance", DISTANCE_FORMAT),
        );
        commands.entity(entity).insert(target);
    }
}

fn prepare_cloud_buffers(
    frame: Res<CloudFrame>,
    mut buffers: ResMut<CloudBuffers>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    if frame.volumes.is_empty() {
        return;
    }
    buffers.globals.set(frame.globals);
    buffers.globals.write_buffer(&render_device, &render_queue);
    buffers.volumes.set(frame.volumes.clone());
    buffers.volumes.write_buffer(&render_device, &render_queue);
    buffers.spheres.set(frame.spheres.clone());
    buffers.spheres.write_buffer(&render_device, &render_queue);
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_cloud_hulls(
    view: ViewQuery<(
        &ExtractedCamera,
        &ViewTarget,
        &ViewDepthTexture,
        &ViewUniformOffset,
        &ViewCloudPipelines,
        &Msaa,
        &ViewCloudTarget,
    )>,
    frame: Res<CloudFrame>,
    pipelines: Option<Res<CloudPipelines>>,
    buffers: Option<Res<CloudBuffers>>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    mut ctx: RenderContext,
) {
    let (Some(pipelines), Some(buffers)) = (pipelines, buffers) else {
        return;
    };
    if frame.volumes.is_empty() {
        return;
    }
    let (camera, target, depth, view_offset, ids, msaa, clouds) = view.into_inner();
    let mut hulls = [None; 8];
    for (hull, id) in hulls.iter_mut().zip(ids.0) {
        *hull = pipeline_cache.get_render_pipeline(id);
        if hull.is_none() {
            return;
        }
    }
    let (Some(composite), Some(depth_write)) = (
        pipeline_cache.get_render_pipeline(ids.1[0]),
        pipeline_cache.get_render_pipeline(ids.1[1]),
    ) else {
        return;
    };
    let (Some(view_binding), Some(globals), Some(volumes), Some(spheres)) = (
        view_uniforms.uniforms.binding(),
        buffers.globals.binding(),
        buffers.volumes.binding(),
        buffers.spheres.binding(),
    ) else {
        return;
    };

    // March into the cloud textures (the scene's depth is read here).
    let bind_group = ctx.render_device().create_bind_group(
        "cloud_hull_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipelines.layouts[(msaa.samples() > 1) as usize]),
        &BindGroupEntries::sequential((
            depth.view(),
            view_binding.clone(),
            globals.clone(),
            volumes,
            spheres,
        )),
    );
    let cleared = |view, clear| RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: Operations { load: LoadOp::Clear(clear), store: StoreOp::Store },
    };
    let no_cloud = LinearRgba::gray(NO_CLOUD).into();
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("malvar_ridge_hull_pass"),
        color_attachments: &[
            Some(cleared(&clouds.0.default_view, LinearRgba::NONE.into())),
            Some(cleared(&clouds.1.default_view, no_cloud)),
        ],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    for (batch, range) in frame.batches.iter().enumerate() {
        if range.y == 0 {
            continue;
        }
        pass.set_render_pipeline(hulls[batch].unwrap());
        pass.draw(0..36, range.x..(range.x + range.y));
    }
    drop(pass);

    // Over the view, then into its depth buffer: transparent geometry behind the clouds (light
    // dots, smoke) is hidden by them.
    let bind_group = ctx.render_device().create_bind_group(
        "cloud_composite_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipelines.composite_layout),
        &BindGroupEntries::sequential((
            &clouds.0.default_view,
            if frame.dither { &pipelines.nearest } else { &pipelines.linear },
            view_binding,
            globals,
            &clouds.1.default_view,
        )),
    );
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("cloud_composite_pass"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(viewport) = camera.viewport.as_ref() {
        pass.set_camera_viewport(viewport);
    }
    pass.set_bind_group(0, &bind_group, &[view_offset.offset]);
    pass.set_render_pipeline(composite);
    pass.draw(0..3, 0..1);
    pass.set_render_pipeline(depth_write);
    pass.draw(0..3, 0..1);
}
