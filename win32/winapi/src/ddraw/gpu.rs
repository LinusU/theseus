//! The wgpu side of Direct3D: an offscreen render target that triangles are
//! drawn into, textures, and reading the result back.
//!
//! This knows nothing about DirectDraw objects or guest memory. The render
//! target can be larger than the game's back buffer (`THESEUS_D3D_SCALE`),
//! in which case reading it back averages each block of pixels down to one,
//! so the game still sees its 640x480 buffer but with antialiased edges and
//! smoother texture filtering.
//!
//! Per-triangle fixed-function state (texture blend mode, fog, alpha test,
//! color key) rides along in each vertex, so only real pipeline state (blend
//! factors, depth, culling) and texture changes split a batch.

#[cfg(not(target_family = "wasm"))]
use std::collections::HashMap;
use std::ops::Range;

/// A vertex as the shader sees it, already in clip space.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 4],
    pub color: [f32; 4],
    pub specular: [f32; 4],
    pub uv: [f32; 2],
    /// `flags::*`.
    pub flags: u32,
    /// ALPHAREF, 0..1.
    pub alpha_ref: f32,
    pub fog_color: [f32; 3],
}

/// Bits of `Vertex::flags`.
pub mod flags {
    /// D3DTBLEND_* in the low four bits.
    pub const BLEND_MASK: u32 = 0xf;
    pub const TEXTURED: u32 = 1 << 4;
    pub const TEXTURE_ALPHA: u32 = 1 << 5;
    pub const SPECULAR: u32 = 1 << 6;
    pub const FOG: u32 = 1 << 7;
    pub const ALPHA_TEST: u32 = 1 << 8;
    /// D3DCMP_* for the alpha test.
    pub const ALPHA_FUNC_SHIFT: u32 = 9;
    pub const COLOR_KEY: u32 = 1 << 13;
}

#[cfg(not(target_family = "wasm"))]
const SHADER: &str = r#"
struct VIn {
    @location(0) pos: vec4f,
    @location(1) color: vec4f,
    @location(2) specular: vec4f,
    @location(3) uv: vec2f,
    @location(4) flags: u32,
    @location(5) alpha_ref: f32,
    @location(6) fog_color: vec3f,
};
struct VOut {
    @builtin(position) pos: vec4f,
    @location(0) color: vec4f,
    @location(1) specular: vec4f,
    @location(2) uv: vec2f,
    @location(3) @interpolate(flat) flags: u32,
    @location(4) @interpolate(flat) alpha_ref: f32,
    @location(5) @interpolate(flat) fog_color: vec3f,
};
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@vertex fn vs(v: VIn) -> VOut {
    return VOut(v.pos, v.color, v.specular, v.uv, v.flags, v.alpha_ref, v.fog_color);
}

fn alpha_passes(func: u32, a: f32, r: f32) -> bool {
    switch func {
        case 1u: { return false; }
        case 2u: { return a < r; }
        case 3u: { return a == r; }
        case 4u: { return a <= r; }
        case 5u: { return a > r; }
        case 6u: { return a != r; }
        case 7u: { return a >= r; }
        default: { return true; }
    }
}

@fragment fn fs(f: VOut) -> @location(0) vec4f {
    // Always sample (an untextured draw binds a white texel): sampling must
    // not depend on per-fragment branches.
    let t = textureSample(tex, samp, f.uv);
    let c = f.color;
    if (f.flags & (1u << 13u)) != 0u && t.a == 0.0 {
        discard;
    }
    var out: vec4f;
    let tex_alpha = (f.flags & (1u << 5u)) != 0u;
    switch f.flags & 0xfu {
        case 1u, 7u: { // DECAL, COPY
            out = t;
        }
        case 3u: { // DECALALPHA
            out = vec4f(mix(c.rgb, t.rgb, t.a), c.a);
        }
        case 4u: { // MODULATEALPHA
            out = t * c;
        }
        case 8u: { // ADD
            out = vec4f(t.rgb + c.rgb, c.a);
        }
        default: { // MODULATE
            out = vec4f(t.rgb * c.rgb, select(c.a, t.a, tex_alpha));
        }
    }
    if (f.flags & (1u << 6u)) != 0u {
        out = vec4f(out.rgb + f.specular.rgb, out.a);
    }
    if (f.flags & (1u << 7u)) != 0u {
        out = vec4f(mix(f.fog_color, out.rgb, f.specular.a), out.a);
    }
    if (f.flags & (1u << 8u)) != 0u {
        if !alpha_passes((f.flags >> 9u) & 0xfu, out.a, f.alpha_ref) {
            discard;
        }
    }
    return clamp(out, vec4f(0.0), vec4f(1.0));
}
"#;

/// Render state that needs its own pipeline.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PipelineKey {
    /// D3DBLEND_* source and destination factors, when blending.
    pub blend: Option<(u8, u8)>,
    /// D3DCMP_* depth test, when depth testing.
    pub depth_test: Option<u8>,
    pub depth_write: bool,
    pub color_write: bool,
    /// D3DCULL_*.
    pub cull: u8,
}

impl PipelineKey {
    /// Overwrite everything a draw covers: for clears and uploads.
    pub const REPLACE: PipelineKey = PipelineKey {
        blend: None,
        depth_test: None,
        depth_write: false,
        color_write: true,
        cull: 1,
    };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SamplerKey {
    pub linear_mag: bool,
    pub linear_min: bool,
    /// D3DTADDRESS_*.
    pub address: u8,
}

impl SamplerKey {
    pub const NEAREST: SamplerKey = SamplerKey {
        linear_mag: false,
        linear_min: false,
        address: 3,
    };
}

/// A run of vertices (three per triangle) drawn with one state.
pub struct Batch {
    pub pipeline: PipelineKey,
    /// A texture from `Gpu::texture`, or none for white.
    pub texture: Option<u64>,
    pub sampler: SamplerKey,
    pub range: Range<u32>,
}

#[cfg(not(target_family = "wasm"))]
struct Texture {
    generation: u64,
    view: wgpu::TextureView,
    /// Bind groups for the samplers this texture has been drawn with.
    bind_groups: HashMap<SamplerKey, wgpu::BindGroup>,
}

#[cfg(not(target_family = "wasm"))]
struct Target {
    /// The game's size.
    width: u32,
    height: u32,
    /// The render target is this many times larger in each direction.
    scale: u32,
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    padded_row: u32,
    /// The game's size, for uploading what it drew to the back buffer.
    staging: wgpu::Texture,
}

#[cfg(not(target_family = "wasm"))]
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    shader: wgpu::ShaderModule,
    bind_layout: wgpu::BindGroupLayout,
    layout: wgpu::PipelineLayout,
    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
    textures: HashMap<u64, Texture>,
    vertex_buffer: Option<wgpu::Buffer>,
    target: Option<Target>,
}

/// Texture key reserved for the white texel untextured draws sample.
#[cfg(not(target_family = "wasm"))]
const WHITE: u64 = u64::MAX;
/// Texture key reserved for the staging texture of an upload.
#[cfg(not(target_family = "wasm"))]
const STAGING: u64 = u64::MAX - 1;

#[cfg(not(target_family = "wasm"))]
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
#[cfg(not(target_family = "wasm"))]
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

#[cfg(not(target_family = "wasm"))]
fn blend_factor(d3d: u8) -> wgpu::BlendFactor {
    use wgpu::BlendFactor as B;
    match d3d {
        1 => B::Zero,
        2 => B::One,
        3 => B::Src,
        4 => B::OneMinusSrc,
        5 | 12 => B::SrcAlpha,
        6 | 13 => B::OneMinusSrcAlpha,
        7 => B::DstAlpha,
        8 => B::OneMinusDstAlpha,
        9 => B::Dst,
        10 => B::OneMinusDst,
        11 => B::SrcAlphaSaturated,
        _ => B::One,
    }
}

#[cfg(not(target_family = "wasm"))]
fn compare(d3d: u8) -> wgpu::CompareFunction {
    use wgpu::CompareFunction as C;
    match d3d {
        1 => C::Never,
        2 => C::Less,
        3 => C::Equal,
        4 => C::LessEqual,
        5 => C::Greater,
        6 => C::NotEqual,
        7 => C::GreaterEqual,
        _ => C::Always,
    }
}

#[cfg(not(target_family = "wasm"))]
fn scale_from_env() -> u32 {
    if let Some(scale) = std::env::var("THESEUS_D3D_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
    {
        return u32::clamp(scale, 1, 8);
    }
    2
}

#[cfg(not(target_family = "wasm"))]
impl Gpu {
    pub fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = match pollster::block_on(
            instance.request_adapter(&wgpu::RequestAdapterOptions::default()),
        ) {
            Ok(adapter) => adapter,
            Err(err) => {
                log::warn!("d3d: no GPU adapter: {err}");
                return None;
            }
        };
        log::info!("d3d: rendering with {:?}", adapter.get_info().name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("d3d"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("d3d texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("d3d"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });

        let mut gpu = Gpu {
            device,
            queue,
            shader,
            bind_layout,
            layout,
            pipelines: HashMap::new(),
            samplers: HashMap::new(),
            textures: HashMap::new(),
            vertex_buffer: None,
            target: None,
        };
        gpu.texture(WHITE, 0, 1, 1, || vec![255; 4]);
        Some(gpu)
    }

    /// Make the render target match the game's back buffer size.
    pub fn set_target_size(&mut self, width: u32, height: u32) {
        if let Some(target) = &self.target {
            if (target.width, target.height) == (width, height) {
                return;
            }
        }
        let scale = scale_from_env();
        let (w, h) = (width * scale, height * scale);
        let texture = |format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("d3d target"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            COLOR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let padded_row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("d3d readback"),
            size: (padded_row * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("d3d staging"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.textures.insert(
            STAGING,
            Texture {
                generation: 0,
                view: staging.create_view(&Default::default()),
                bind_groups: HashMap::new(),
            },
        );
        log::info!("d3d: {width}x{height} target rendered at {w}x{h}");
        self.target = Some(Target {
            width,
            height,
            scale,
            color_view: color.create_view(&Default::default()),
            color,
            depth_view: depth.create_view(&Default::default()),
            readback,
            padded_row,
            staging,
        });
    }

    /// Make sure a texture is uploaded. `key` identifies it across calls;
    /// `pixels` (RGBA) is only asked for when `generation` changed.
    pub fn texture(
        &mut self,
        key: u64,
        generation: u64,
        width: u32,
        height: u32,
        pixels: impl FnOnce() -> Vec<u8>,
    ) {
        if let Some(texture) = self.textures.get(&key) {
            if texture.generation == generation {
                return;
            }
        }
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("d3d texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            &pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            size,
        );
        self.textures.insert(
            key,
            Texture {
                generation,
                view: texture.create_view(&Default::default()),
                bind_groups: HashMap::new(),
            },
        );
    }

    fn pipeline(&mut self, key: PipelineKey) -> &wgpu::RenderPipeline {
        let device = &self.device;
        let (shader, layout) = (&self.shader, &self.layout);
        self.pipelines.entry(key).or_insert_with(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("d3d"),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2,
                            4 => Uint32, 5 => Float32, 6 => Float32x3
                        ],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: key.blend.map(|(src, dst)| {
                            let component = wgpu::BlendComponent {
                                src_factor: blend_factor(src),
                                dst_factor: blend_factor(dst),
                                operation: wgpu::BlendOperation::Add,
                            };
                            wgpu::BlendState {
                                color: component,
                                alpha: component,
                            }
                        }),
                        write_mask: if key.color_write {
                            wgpu::ColorWrites::ALL
                        } else {
                            wgpu::ColorWrites::empty()
                        },
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    // Both name the winding as it appears on screen, so D3D's
                    // counter-clockwise is wgpu's too (the y flip into clip
                    // space doesn't change the picture).
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: match key.cull {
                        2 => Some(wgpu::Face::Back),  // D3DCULL_CW
                        3 => Some(wgpu::Face::Front), // D3DCULL_CCW
                        _ => None,
                    },
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(key.depth_write),
                    depth_compare: Some(key.depth_test.map_or(wgpu::CompareFunction::Always, compare)),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    fn bind_group(&mut self, texture: u64, sampler: SamplerKey) -> Option<wgpu::BindGroup> {
        let device = &self.device;
        let sampler_obj = self.samplers.entry(sampler).or_insert_with(|| {
            let address = match sampler.address {
                2 => wgpu::AddressMode::MirrorRepeat,
                3 => wgpu::AddressMode::ClampToEdge,
                _ => wgpu::AddressMode::Repeat,
            };
            let filter = |linear| {
                if linear {
                    wgpu::FilterMode::Linear
                } else {
                    wgpu::FilterMode::Nearest
                }
            };
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("d3d"),
                address_mode_u: address,
                address_mode_v: address,
                mag_filter: filter(sampler.linear_mag),
                min_filter: filter(sampler.linear_min),
                ..Default::default()
            })
        });
        let layout = &self.bind_layout;
        let texture = self.textures.get_mut(&texture)?;
        let view = &texture.view;
        Some(
            texture
                .bind_groups
                .entry(sampler)
                .or_insert_with(|| {
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("d3d"),
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(sampler_obj),
                            },
                        ],
                    })
                })
                .clone(),
        )
    }

    /// Draw batches of triangles over what the target already holds.
    pub fn draw(&mut self, vertices: &[Vertex], batches: &[Batch]) {
        if vertices.is_empty() || self.target.is_none() {
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(vertices);
        let too_small = self
            .vertex_buffer
            .as_ref()
            .is_none_or(|b| b.size() < bytes.len() as u64);
        if too_small {
            self.vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("d3d vertices"),
                size: (bytes.len() as u64).next_power_of_two().max(1 << 16),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let vertex_buffer = self.vertex_buffer.clone().unwrap();
        self.queue.write_buffer(&vertex_buffer, 0, bytes);

        // Resolve every pipeline and bind group first; the pass borrows them.
        let mut draws = Vec::with_capacity(batches.len());
        for batch in batches {
            let pipeline = self.pipeline(batch.pipeline).clone();
            let Some(bind_group) = self
                .bind_group(batch.texture.unwrap_or(WHITE), batch.sampler)
                .or_else(|| self.bind_group(WHITE, batch.sampler))
            else {
                continue;
            };
            draws.push((pipeline, bind_group, batch.range.clone()));
        }

        let target = self.target.as_ref().unwrap();
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("d3d"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            for (pipeline, bind_group, range) in &draws {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(range.clone(), 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
    }

    /// Fill a rect (in the game's pixels; None for all of it) with a color
    /// and/or a depth.
    pub fn clear(&mut self, rect: Option<[u32; 4]>, color: Option<[f32; 4]>, depth: Option<f32>) {
        let Some(target) = &self.target else {
            return;
        };
        let [x0, y0, x1, y1] = rect.unwrap_or([0, 0, target.width, target.height]);
        let (w, h) = (target.width as f32, target.height as f32);
        let quad = quad(
            [x0 as f32 / w, y0 as f32 / h, x1 as f32 / w, y1 as f32 / h],
            depth.unwrap_or(0.0),
            color.unwrap_or([0.0; 4]),
            flags::TEXTURED - flags::TEXTURED, // untextured, MODULATE of white
        );
        let pipeline = PipelineKey {
            depth_write: depth.is_some(),
            color_write: color.is_some(),
            ..PipelineKey::REPLACE
        };
        self.draw(
            &quad,
            &[Batch {
                pipeline,
                texture: None,
                sampler: SamplerKey::NEAREST,
                range: 0..6,
            }],
        );
    }

    /// Replace the target's contents with what the game drew itself (RGBA,
    /// the game's size), leaving depth alone.
    ///
    /// With `only_opaque`, pixels with alpha 0 leave the target as it is, so
    /// just what changed can be laid over detail drawn at full resolution.
    pub fn upload(&mut self, rgba: &[u8], only_opaque: bool) {
        let Some(target) = &self.target else {
            return;
        };
        self.queue.write_texture(
            target.staging.as_image_copy(),
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(target.width * 4),
                rows_per_image: Some(target.height),
            },
            wgpu::Extent3d {
                width: target.width,
                height: target.height,
                depth_or_array_layers: 1,
            },
        );
        let keyed = if only_opaque { flags::COLOR_KEY } else { 0 };
        let quad = quad([0.0, 0.0, 1.0, 1.0], 0.0, [1.0; 4], 1 /* DECAL */ | keyed);
        self.draw(
            &quad,
            &[Batch {
                pipeline: PipelineKey::REPLACE,
                texture: Some(STAGING),
                sampler: SamplerKey::NEAREST,
                range: 0..6,
            }],
        );
    }

    /// Read the target back at the game's size (RGBA), averaging each
    /// scale x scale block.
    pub fn read(&mut self) -> Option<Vec<u8>> {
        let (full, width, height, scale) = self.read_full()?;
        Some(downsample(&full, width / scale, height / scale, scale))
    }

    /// Read the whole target back (RGBA), with its width, height and scale.
    pub fn read_full(&mut self) -> Option<(Vec<u8>, u32, u32, u32)> {
        let target = self.target.as_ref()?;
        let (scale, w, h) = (target.scale, target.width, target.height);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            target.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &target.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(target.padded_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: w * scale,
                height: h * scale,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = target.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| {
            if let Err(err) = result {
                log::error!("d3d: readback failed: {err}");
            }
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .ok()?;
        let (full_w, full_h) = ((w * scale) as usize, (h * scale) as usize);
        let mut out = Vec::with_capacity(full_w * full_h * 4);
        {
            let data = slice.get_mapped_range().ok()?;
            let row = target.padded_row as usize;
            for y in 0..full_h {
                out.extend_from_slice(&data[y * row..][..full_w * 4]);
            }
        }
        target.readback.unmap();
        Some((out, full_w as u32, full_h as u32, scale))
    }
}

/// Shrink an RGBA image `scale` times in each direction to `width` x
/// `height`, averaging each block.
pub fn downsample(full: &[u8], width: u32, height: u32, scale: u32) -> Vec<u8> {
    let (w, h, s) = (width as usize, height as usize, scale as usize);
    let row = w * s * 4;
    let div = (s * s) as u32;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..s {
                let base = (y * s + dy) * row + x * s * 4;
                for dx in 0..s {
                    let p = &full[base + dx * 4..][..4];
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for c in 0..4 {
                out[o + c] = ((sum[c] + div / 2) / div) as u8;
            }
        }
    }
    out
}

/// Two triangles covering a rect given as fractions of the target
/// (left, top, right, bottom; y down).
#[cfg(not(target_family = "wasm"))]
fn quad(rect: [f32; 4], depth: f32, color: [f32; 4], flags: u32) -> [Vertex; 6] {
    let [l, t, r, b] = rect;
    let v = |x: f32, y: f32| Vertex {
        pos: [x * 2.0 - 1.0, 1.0 - y * 2.0, depth, 1.0],
        color,
        specular: [0.0, 0.0, 0.0, 1.0],
        uv: [x, y],
        flags,
        ..Default::default()
    };
    [v(l, t), v(l, b), v(r, t), v(r, t), v(l, b), v(r, b)]
}

/// Without wgpu (the web build, for now) there is no GPU: `Gpu::new` fails,
/// and Direct3D devices draw nothing.
#[cfg(target_family = "wasm")]
pub struct Gpu;

#[cfg(target_family = "wasm")]
impl Gpu {
    pub fn new() -> Option<Gpu> {
        None
    }
    pub fn set_target_size(&mut self, _width: u32, _height: u32) {}
    pub fn texture(
        &mut self,
        _key: u64,
        _generation: u64,
        _width: u32,
        _height: u32,
        _pixels: impl FnOnce() -> Vec<u8>,
    ) {
    }
    pub fn draw(&mut self, _vertices: &[Vertex], _batches: &[Batch]) {}
    pub fn clear(&mut self, _rect: Option<[u32; 4]>, _color: Option<[f32; 4]>, _depth: Option<f32>) {}
    pub fn upload(&mut self, _rgba: &[u8], _only_opaque: bool) {}
    pub fn read(&mut self) -> Option<Vec<u8>> {
        None
    }
    pub fn read_full(&mut self) -> Option<(Vec<u8>, u32, u32, u32)> {
        None
    }
}
