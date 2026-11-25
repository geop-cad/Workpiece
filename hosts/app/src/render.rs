//! The viewport's OpenGL renderer, drawn inside egui through a paint
//! callback.
//!
//! Scenes are uploaded once per change (see [`SceneData`]); each frame
//! draws, in order: opaque faces, lines and vertices with depth; then the
//! translucent rest (ghosts, filled planes) without writing depth; then
//! datum frames and the handles on top of everything. Lines and vertices are the quads of
//! [`workpiece_rasterize::quads`], pushed out to their pixel width in the
//! vertex shader. Highlighting happens in the shaders, by pick id.
//!
//! Every OpenGL call is `unsafe` in glow, so this module allows unsafe
//! code; it only passes data glow checks the sizes of.

#![allow(unsafe_code)]

use eframe::egui;
use eframe::glow::{self, HasContext};
use workpiece_rasterize::{Scene, quads};

/// One scene as GPU vertex data.
#[derive(Debug, Default)]
pub struct SceneData {
    opaque: Vec<f32>,
    translucent: Vec<f32>,
    lines: Vec<f32>,
    vertices: Vec<f32>,
    /// What shows in front of the rest: filled planes, lines, vertices.
    front: [Vec<f32>; 3],
}

impl SceneData {
    /// The vertex data of `scene`.
    pub fn new(scene: &Scene) -> Self {
        Self {
            opaque: quads::mesh(&scene.triangles.triangles, false),
            translucent: quads::mesh(&scene.triangles.triangles, true),
            lines: quads::lines(&scene.lines.lines),
            vertices: quads::vertices(&scene.vertices.vertices),
            front: [
                quads::mesh(&scene.front.triangles, true),
                quads::lines(&scene.front.lines),
                quads::vertices(&scene.front.vertices),
            ],
        }
    }
}

/// How many selected picks the shaders highlight.
pub const MAX_SELECTED: usize = 16;

/// The pick ids to highlight in one scene (`-1` for none).
#[derive(Debug, Clone, Copy)]
pub struct Highlight {
    /// Under the pointer.
    pub hovered: f32,
    /// Selected (the first [`MAX_SELECTED`]); `-1` fills the rest.
    pub selected: [f32; MAX_SELECTED],
}

impl Highlight {
    /// Nothing highlighted.
    pub const NONE: Self = Self {
        hovered: -1.0,
        selected: [-1.0; MAX_SELECTED],
    };

    /// `hovered` and the ids in `selected`.
    pub fn new(hovered: f32, selected: impl IntoIterator<Item = f32>) -> Self {
        let mut ids = [-1.0; MAX_SELECTED];
        for (slot, id) in ids.iter_mut().zip(selected) {
            *slot = id;
        }
        Self {
            hovered,
            selected: ids,
        }
    }
}

/// A scene to draw: its data, an id that changes when it does, and what
/// to highlight in it.
pub struct Layer {
    /// Changes whenever `data` does.
    pub id: u64,
    /// The vertex data.
    pub data: std::sync::Arc<SceneData>,
    /// What to highlight.
    pub highlight: Highlight,
    /// Where the scene is: its coordinates to the view's.
    pub model: glam::Mat4,
    /// What is added to the scene's pick ids before they are compared
    /// with the highlight's.
    pub pick_base: f32,
}

/// Everything one frame draws.
pub struct Frame {
    /// World to clip space.
    pub view_proj: glam::Mat4,
    /// The camera's position.
    pub eye: glam::Vec3,
    /// From the target towards the camera.
    pub back: glam::Vec3,
    /// Parallel projection.
    pub orthographic: bool,
    /// What faces are painted to show: 0 nothing, 1 zebra stripes, 2
    /// how they bend, 3 their draft from z.
    pub analysis: f32,
    /// About how big what is looked at is: what a bend is tight beside.
    pub reach: f32,
    /// The background, as the sRGB bytes egui uses.
    pub background: egui::Color32,
    /// The focused part: its own scene and those of what it places,
    /// each where it is.
    pub scene: Vec<Layer>,
    /// The parts around it.
    pub ghosts: Vec<Layer>,
    /// The edited step's handles.
    pub handles: Layer,
}

/// Colors, linear RGBA.
const HOVER: [f32; 4] = workpiece_rasterize::palette::HOVER;
const SELECTION: [f32; 4] = workpiece_rasterize::palette::SELECTION;
const HANDLE_HOVER: [f32; 4] = workpiece_rasterize::palette::HANDLE_HOVER;

const COMMON: &str = r"
#ifdef GL_ES
precision highp float;
#endif
uniform mat4 u_view_proj;
uniform mat4 u_model;
uniform float u_pick_base;
uniform vec2 u_viewport;
uniform vec3 u_eye;
uniform vec3 u_back;
uniform float u_ortho;
uniform float u_analysis;
uniform float u_reach;
uniform float u_hovered;
uniform float u_selected[16];
uniform vec4 u_hover_color;
uniform float u_bias;

vec4 highlight(vec4 c, float own) {
    // The scene's own id, moved to where the scene is in the drawing.
    float pick = own >= 0.0 ? own + u_pick_base : own;
    if (pick >= 0.0) {
        for (int i = 0; i < 16; i++) {
            if (abs(pick - u_selected[i]) < 0.5) {
                return vec4(SELECTION, max(c.a, 0.5));
            }
        }
    }
    if (pick >= 0.0 && abs(pick - u_hovered) < 0.5) {
        return vec4(u_hover_color.rgb, max(c.a, 0.5));
    }
    return c;
}

vec3 to_srgb(vec3 c) {
    vec3 lo = c * 12.92;
    vec3 hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(hi, lo, vec3(lessThanEqual(c, vec3(0.0031308))));
}
";

const MESH_VS: &str = r"
in vec3 a_position;
in vec3 a_normal;
in vec4 a_color;
in float a_pick;
in vec2 a_texture;
out vec3 v_normal;
out vec3 v_world;
out vec3 v_own;
out vec3 v_own_normal;
out vec4 v_color;
out vec2 v_texture;
void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world = world.xyz;
    // (A texture stays on the part as the part is moved: it is worked
    // out where the face is in the part.)
    v_own = a_position;
    v_own_normal = a_normal;
    v_texture = a_texture;
    v_normal = mat3(u_model) * a_normal;
    v_color = highlight(a_color, a_pick);
    gl_Position = u_view_proj * world;
}
";

const MESH_FS: &str = r"
in vec3 v_normal;
in vec3 v_world;
in vec3 v_own;
in vec3 v_own_normal;
in vec4 v_color;
in vec2 v_texture;
out vec4 f_color;

float hash2(vec2 p) {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise2(vec2 p) {
    vec2 i = floor(p);
    vec2 f = fract(p);
    vec2 u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash2(i), hash2(i + vec2(1.0, 0.0)), u.x),
               mix(hash2(i + vec2(0.0, 1.0)), hash2(i + vec2(1.0, 1.0)), u.x), u.y);
}

// How much lighter or darker the texture `kind` makes the face at `p`
// of its own plane (in repeats of its pattern): a factor about 1.
float pattern(float kind, vec2 p) {
    if (kind < 1.5) {
        // Brushed: fine streaks one way.
        return 0.94 + 0.12 * noise2(vec2(p.x * 3.0, p.y * 160.0));
    } else if (kind < 2.5) {
        // Wood: rings, wavering, and a fine grain along them.
        float rings = fract(p.y * 6.0 + 0.6 * noise2(p * vec2(1.5, 4.0)));
        return 0.80 + 0.22 * smoothstep(0.0, 0.7, rings) * (1.0 - smoothstep(0.85, 1.0, rings))
            + 0.05 * noise2(vec2(p.x * 80.0, p.y * 8.0));
    } else if (kind < 3.5) {
        // Weave: tows over and under each other.
        vec2 cell = floor(p * 4.0);
        vec2 f = fract(p * 4.0);
        float over = mod(cell.x + cell.y, 2.0);
        float across = over > 0.5 ? f.x : f.y;
        return 0.75 + 0.45 * sin(across * 3.14159) * (0.8 + 0.2 * over);
    } else if (kind < 4.5) {
        // Checker.
        vec2 cell = floor(p * 2.0);
        return mod(cell.x + cell.y, 2.0) > 0.5 ? 1.08 : 0.80;
    } else if (kind < 5.5) {
        // Speckle.
        return 0.86 + 0.24 * noise2(p * 40.0) * noise2(p * 13.0 + 7.0);
    }
    // Ribbed.
    return 0.82 + 0.22 * smoothstep(0.2, 0.5, abs(fract(p.x * 4.0) - 0.5) * 2.0);
}

void main() {
    vec3 v = u_ortho > 0.5 ? normalize(u_back) : normalize(u_eye - v_world);
    vec3 n = normalize(v_normal);
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    // Hemisphere ambient, a key light from over the viewer's shoulder, a
    // fill, specular and a rim.
    vec3 key = normalize(v + vec3(0.25, 0.15, 0.55));
    vec3 fill = normalize(vec3(-key.x, -key.y, 0.3));
    vec3 ambient = mix(vec3(0.30, 0.29, 0.28), vec3(0.62, 0.66, 0.72), n.z * 0.5 + 0.5);
    float diffuse = max(dot(n, key), 0.0);
    float fill_light = max(dot(n, fill), 0.0);
    vec3 h = normalize(key + v);
    float specular = pow(max(dot(n, h), 0.0), 64.0) * 0.25;
    float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.12;
    vec3 base = v_color.rgb;
    if (v_texture.x > 0.5) {
        // In the plane the face most nearly lies in, of the part's own.
        vec3 a = abs(normalize(v_own_normal));
        vec2 p = a.z >= a.x && a.z >= a.y ? v_own.xy : (a.y >= a.x ? v_own.xz : v_own.yz);
        base *= pattern(v_texture.x, p / max(v_texture.y, 1e-6));
    }
    vec3 rgb = base * (ambient * 0.68 + diffuse * 0.52 + fill_light * 0.16) + vec3(specular + rim);
    if (u_analysis > 0.5 && u_analysis < 1.5) {
        // Stripes of a room mirrored in the face.
        vec3 r = reflect(-v, n);
        // Sixteen stripes around (a whole number, so none breaks where
        // the angle starts over), their borders a pixel soft.
        float along = atan(r.y, r.x) * 2.5464790895 + r.z * 3.0;
        float soft = clamp(fwidth(along), 1e-4, 0.25);
        float stripe = smoothstep(-soft, soft, 0.25 - abs(fract(along) - 0.75));
        rgb = mix(vec3(0.02), vec3(0.9), stripe) * (0.55 + 0.45 * diffuse);
    } else if (u_analysis > 1.5 && u_analysis < 2.5) {
        // How fast the normal turns across the face, beside the size
        // of what is looked at: blue flat, through green, to red.
        float bend = length(fwidth(normalize(v_normal))) / max(length(fwidth(v_world)), 1e-12);
        float t = clamp(log(1.0 + bend * u_reach) / log(41.0), 0.0, 1.0);
        vec3 hue = clamp(vec3(2.0 * t - 1.0, 1.0 - abs(2.0 * t - 1.0), 1.0 - 2.0 * t), 0.0, 1.0);
        rgb = hue * (0.45 + 0.55 * diffuse);
    } else if (u_analysis > 2.5) {
        // The lean of the face as it is, from a pull along z: none or
        // undercut red, under 3 degrees yellow, more green.
        float lean = degrees(asin(clamp(normalize(v_normal).z, -1.0, 1.0)));
        vec3 hue = lean < 0.05 ? vec3(0.8, 0.1, 0.08) : (lean < 3.0 ? vec3(0.85, 0.7, 0.05) : vec3(0.1, 0.6, 0.15));
        rgb = hue * (0.45 + 0.55 * diffuse);
    }
    f_color = vec4(to_srgb(rgb), v_color.a);
}
";

const LINE_VS: &str = r"
in vec3 a_a;
in vec3 a_b;
in vec2 a_corner;
in vec2 a_width;
in vec4 a_color;
in float a_pick;
out vec4 v_color;
out vec2 v_side;

// How far outside the view (in view sizes) lines are kept.
const float GUARD = 3.0;

// How far `p` is inside clip plane `k`: the near plane, then a guard band.
float inside(vec4 p, int k) {
    if (k == 0) { return p.z + p.w; }
    if (k == 1) { return GUARD * p.w - p.x; }
    if (k == 2) { return GUARD * p.w + p.x; }
    if (k == 3) { return GUARD * p.w - p.y; }
    return GUARD * p.w + p.y;
}

void main() {
    vec4 ca = u_view_proj * (u_model * vec4(a_a, 1.0));
    vec4 cb = u_view_proj * (u_model * vec4(a_b, 1.0));
    // Clip to the near plane and a guard band, so lines reaching behind
    // the camera neither flip nor become huge.
    float t0 = 0.0;
    float t1 = 1.0;
    for (int k = 0; k < 5; k++) {
        float fa = inside(ca, k);
        float fb = inside(cb, k);
        if (fa < 0.0 && fb < 0.0) {
            t1 = -1.0;
        } else if (fa < 0.0) {
            t0 = max(t0, fa / (fa - fb));
        } else if (fb < 0.0) {
            t1 = min(t1, fa / (fa - fb));
        }
    }
    v_color = highlight(a_color, a_pick);
    if (t0 > t1) {
        gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
        v_side = vec2(0.0);
        return;
    }
    vec4 a = mix(ca, cb, t0);
    vec4 b = mix(ca, cb, t1);
    vec2 half_px = u_viewport * 0.5;
    vec2 sa = a.xy / max(a.w, 1e-6) * half_px;
    vec2 sb = b.xy / max(b.w, 1e-6) * half_px;
    vec2 dir = sb - sa;
    dir = length(dir) < 1e-6 ? vec2(1.0, 0.0) : normalize(dir);
    vec2 normal = vec2(-dir.y, dir.x);
    float width = mix(a_width.x, a_width.y, mix(t0, t1, a_corner.x));
    // One pixel more on each side and end, for anti-aliasing.
    float reach = width * 0.5 + 1.0;
    vec4 clip = mix(a, b, a_corner.x);
    vec2 offset = normal * a_corner.y * reach + dir * (a_corner.x * 2.0 - 1.0);
    clip.xy += offset / half_px * clip.w;
    // In front of the faces the lines lie on.
    clip.z -= u_bias * clip.w;
    gl_Position = clip;
    v_side = vec2(a_corner.y * reach, width * 0.5);
}
";

const LINE_FS: &str = r"
in vec4 v_color;
in vec2 v_side;
out vec4 f_color;
void main() {
    float coverage = clamp(v_side.y + 0.5 - abs(v_side.x), 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    f_color = vec4(to_srgb(v_color.rgb), v_color.a * coverage);
}
";

const POINT_VS: &str = r"
in vec3 a_center;
in vec2 a_corner;
in float a_size;
in vec4 a_color;
in float a_pick;
out vec4 v_color;
out vec3 v_side;
void main() {
    float reach = a_size * 0.5 + 1.0;
    vec2 offset = a_corner * reach;
    vec4 clip = u_view_proj * (u_model * vec4(a_center, 1.0));
    // In front of the lines and faces the vertex lies on.
    clip.z -= 2.0 * u_bias * clip.w;
    clip.xy += offset / (u_viewport * 0.5) * clip.w;
    gl_Position = clip;
    v_color = highlight(a_color, a_pick);
    v_side = vec3(offset, a_size * 0.5);
}
";

const POINT_FS: &str = r"
in vec4 v_color;
in vec3 v_side;
out vec4 f_color;
void main() {
    float r = length(v_side.xy);
    float radius = v_side.z;
    float coverage = clamp(radius + 0.5 - r, 0.0, 1.0);
    if (coverage <= 0.0) {
        discard;
    }
    // A light center in a colored ring.
    float ring = 1.0 - clamp(radius * 0.45 + 0.5 - r, 0.0, 1.0);
    vec3 rgb = mix(vec3(0.95, 0.96, 0.97), v_color.rgb, ring);
    f_color = vec4(to_srgb(rgb), v_color.a * coverage);
}
";

/// A linked program and the names of its attributes, by location.
struct Program {
    program: glow::Program,
    /// (size in floats) per attribute location.
    layout: &'static [i32],
}

/// A vertex buffer and how many vertices it holds.
struct Buffer {
    vbo: glow::Buffer,
    floats: usize,
}

/// One uploaded scene.
struct Uploaded {
    id: u64,
    opaque: Buffer,
    translucent: Buffer,
    lines: Buffer,
    vertices: Buffer,
    front: [Buffer; 3],
}

/// The renderer's GPU resources.
pub struct Renderer {
    mesh: Program,
    line: Program,
    point: Program,
    vao: glow::VertexArray,
    /// The scenes uploaded, by their id: a scene is uploaded once, and
    /// drawn wherever and as often as it is placed.
    scenes: std::collections::HashMap<u64, Uploaded>,
    handles: Option<Uploaded>,
}

fn compile(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
    attributes: &[&str],
    layout: &'static [i32],
) -> anyhow::Result<Program> {
    let version = eframe::egui_glow::ShaderVersion::get(gl);
    let selection = format!(
        "#define SELECTION vec3({:.4}, {:.4}, {:.4})\n",
        SELECTION[0], SELECTION[1], SELECTION[2]
    );
    let header = format!("{}\n{selection}{COMMON}", version.version_declaration());
    unsafe {
        let program = gl.create_program().map_err(anyhow::Error::msg)?;
        let mut shaders = Vec::new();
        for (kind, source) in [
            (glow::VERTEX_SHADER, vertex),
            (glow::FRAGMENT_SHADER, fragment),
        ] {
            let shader = gl.create_shader(kind).map_err(anyhow::Error::msg)?;
            gl.shader_source(shader, &format!("{header}{source}"));
            gl.compile_shader(shader);
            anyhow::ensure!(
                gl.get_shader_compile_status(shader),
                "shader: {}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        for (i, name) in attributes.iter().enumerate() {
            gl.bind_attrib_location(program, i as u32, name);
        }
        gl.link_program(program);
        anyhow::ensure!(
            gl.get_program_link_status(program),
            "program: {}",
            gl.get_program_info_log(program)
        );
        for shader in shaders {
            gl.detach_shader(program, shader);
            gl.delete_shader(shader);
        }
        Ok(Program { program, layout })
    }
}

impl Renderer {
    /// Compiles the shaders.
    pub fn new(gl: &glow::Context) -> anyhow::Result<Self> {
        let mesh = compile(
            gl,
            MESH_VS,
            MESH_FS,
            &["a_position", "a_normal", "a_color", "a_pick", "a_texture"],
            &[3, 3, 4, 1, 2],
        )?;
        let line = compile(
            gl,
            LINE_VS,
            LINE_FS,
            &["a_a", "a_b", "a_corner", "a_width", "a_color", "a_pick"],
            &[3, 3, 2, 2, 4, 1],
        )?;
        let point = compile(
            gl,
            POINT_VS,
            POINT_FS,
            &["a_center", "a_corner", "a_size", "a_color", "a_pick"],
            &[3, 2, 1, 4, 1],
        )?;
        let vao = unsafe { gl.create_vertex_array().map_err(anyhow::Error::msg)? };
        Ok(Self {
            mesh,
            line,
            point,
            vao,
            scenes: std::collections::HashMap::new(),
            handles: None,
        })
    }

    fn buffer(gl: &glow::Context, data: &[f32], reuse: Option<Buffer>) -> Option<Buffer> {
        unsafe {
            let vbo = match reuse {
                Some(b) => b.vbo,
                None => gl.create_buffer().ok()?,
            };
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                bytemuck::cast_slice(data),
                glow::DYNAMIC_DRAW,
            );
            Some(Buffer {
                vbo,
                floats: data.len(),
            })
        }
    }

    /// Uploads `layer` into `slot` unless it is there already.
    fn upload(gl: &glow::Context, slot: &mut Option<Uploaded>, layer: &Layer) {
        if slot.as_ref().is_some_and(|u| u.id == layer.id) {
            return;
        }
        let old = slot.take();
        let (mut o, mut t, mut l, mut v, f) = match old {
            Some(u) => (
                Some(u.opaque),
                Some(u.translucent),
                Some(u.lines),
                Some(u.vertices),
                u.front.map(Some),
            ),
            None => (None, None, None, None, [None, None, None]),
        };
        let d = &layer.data;
        let [f0, f1, f2] = f;
        let (
            Some(opaque),
            Some(translucent),
            Some(lines),
            Some(vertices),
            Some(f0),
            Some(f1),
            Some(f2),
        ) = (
            Self::buffer(gl, &d.opaque, o.take()),
            Self::buffer(gl, &d.translucent, t.take()),
            Self::buffer(gl, &d.lines, l.take()),
            Self::buffer(gl, &d.vertices, v.take()),
            Self::buffer(gl, &d.front[0], f0),
            Self::buffer(gl, &d.front[1], f1),
            Self::buffer(gl, &d.front[2], f2),
        )
        else {
            return;
        };
        *slot = Some(Uploaded {
            id: layer.id,
            opaque,
            translucent,
            lines,
            vertices,
            front: [f0, f1, f2],
        });
    }

    fn draw(
        gl: &glow::Context,
        program: &Program,
        buffer: &Buffer,
        frame: &Frame,
        viewport: [f32; 2],
        layer: &Layer,
        hover_color: [f32; 4],
    ) {
        let highlight = layer.highlight;
        let stride: i32 = program.layout.iter().sum();
        let vertices = buffer.floats / stride as usize;
        if vertices == 0 {
            return;
        }
        unsafe {
            gl.use_program(Some(program.program));
            let loc = |name: &str| gl.get_uniform_location(program.program, name);
            gl.uniform_matrix_4_f32_slice(
                loc("u_view_proj").as_ref(),
                false,
                &frame.view_proj.to_cols_array(),
            );
            gl.uniform_matrix_4_f32_slice(
                loc("u_model").as_ref(),
                false,
                &layer.model.to_cols_array(),
            );
            gl.uniform_1_f32(loc("u_pick_base").as_ref(), layer.pick_base);
            gl.uniform_2_f32(loc("u_viewport").as_ref(), viewport[0], viewport[1]);
            gl.uniform_3_f32(loc("u_eye").as_ref(), frame.eye.x, frame.eye.y, frame.eye.z);
            gl.uniform_3_f32(
                loc("u_back").as_ref(),
                frame.back.x,
                frame.back.y,
                frame.back.z,
            );
            gl.uniform_1_f32(
                loc("u_ortho").as_ref(),
                if frame.orthographic { 1.0 } else { 0.0 },
            );
            gl.uniform_1_f32(loc("u_analysis").as_ref(), frame.analysis);
            gl.uniform_1_f32(loc("u_reach").as_ref(), frame.reach);
            gl.uniform_1_f32(loc("u_hovered").as_ref(), highlight.hovered);
            gl.uniform_1_f32_slice(loc("u_selected").as_ref(), &highlight.selected);
            let [r, g, b, a] = hover_color;
            gl.uniform_4_f32(loc("u_hover_color").as_ref(), r, g, b, a);
            gl.uniform_1_f32(
                loc("u_bias").as_ref(),
                if frame.orthographic { 1e-4 } else { 4e-5 },
            );

            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer.vbo));
            let mut offset = 0;
            for (i, &size) in program.layout.iter().enumerate() {
                gl.enable_vertex_attrib_array(i as u32);
                gl.vertex_attrib_pointer_f32(
                    i as u32,
                    size,
                    glow::FLOAT,
                    false,
                    stride * 4,
                    offset * 4,
                );
                offset += size;
            }
            gl.draw_arrays(glow::TRIANGLES, 0, vertices as i32);
            for i in 0..program.layout.len() {
                gl.disable_vertex_attrib_array(i as u32);
            }
        }
    }

    /// Draws `frame` into the callback's viewport.
    pub fn paint(&mut self, gl: &glow::Context, info: &egui::PaintCallbackInfo, frame: &Frame) {
        let prof = std::time::Instant::now(); // PROF
        self.paint_(gl, info, frame);
        if std::env::var_os("WP_PROF").is_some() { eprintln!("PROF paint {:>7.2} ms", prof.elapsed().as_secs_f64()*1e3); } // PROF
    }
    fn paint_(&mut self, gl: &glow::Context, info: &egui::PaintCallbackInfo, frame: &Frame) { // PROF
        // What is drawn is uploaded unless it is there; what is no longer
        // drawn is let go.
        for layer in frame.scene.iter().chain(&frame.ghosts) {
            let mut slot = self.scenes.remove(&layer.id);
            Self::upload(gl, &mut slot, layer);
            if let Some(uploaded) = slot {
                self.scenes.insert(layer.id, uploaded);
            }
        }
        let drawn: std::collections::HashSet<u64> = frame
            .scene
            .iter()
            .chain(&frame.ghosts)
            .map(|l| l.id)
            .collect();
        let gone: Vec<u64> = self
            .scenes
            .keys()
            .copied()
            .filter(|id| !drawn.contains(id))
            .collect();
        for id in gone {
            if let Some(old) = self.scenes.remove(&id) {
                let Uploaded {
                    opaque,
                    translucent,
                    lines,
                    vertices,
                    front,
                    ..
                } = old;
                for buffer in [opaque, translucent, lines, vertices]
                    .into_iter()
                    .chain(front)
                {
                    unsafe { gl.delete_buffer(buffer.vbo) };
                }
            }
        }
        Self::upload(gl, &mut self.handles, &frame.handles);
        let px = info.viewport_in_pixels();
        let viewport = [px.width_px as f32, px.height_px as f32];
        let [r, g, b, _] = frame.background.to_normalized_gamma_f32();
        unsafe {
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(px.left_px, px.from_bottom_px, px.width_px, px.height_px);
            gl.clear_color(r, g, b, 1.0);
            gl.depth_mask(true);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.disable(glow::CULL_FACE);
            gl.enable(glow::BLEND);
            gl.blend_func_separate(
                glow::SRC_ALPHA,
                glow::ONE_MINUS_SRC_ALPHA,
                glow::ONE,
                glow::ONE_MINUS_SRC_ALPHA,
            );
            gl.bind_vertex_array(Some(self.vao));
        }
        let scenes = &self.scenes;
        let each = |layers: &[Layer], f: &mut dyn FnMut(&Layer, &Uploaded)| {
            for layer in layers {
                if let Some(uploaded) = scenes.get(&layer.id) {
                    f(layer, uploaded);
                }
            }
        };
        // Faces are drawn a little behind where they are — the more, the
        // more they lean away from the eye: an edge is a strip a few
        // pixels wide at the depth of its middle, and a face that leans
        // away would hide the far half of it, the edge showing thick
        // from one side and thin from another.
        let behind = |on: bool| unsafe {
            if on {
                gl.enable(glow::POLYGON_OFFSET_FILL);
                gl.polygon_offset(2.5, 2.0);
            } else {
                gl.disable(glow::POLYGON_OFFSET_FILL);
            }
        };
        each(&frame.scene, &mut |l, s| {
            behind(true);
            Self::draw(gl, &self.mesh, &s.opaque, frame, viewport, l, HOVER);
            behind(false);
            Self::draw(gl, &self.line, &s.lines, frame, viewport, l, HOVER);
            Self::draw(gl, &self.point, &s.vertices, frame, viewport, l, HOVER);
        });
        unsafe { gl.depth_mask(false) };
        each(&frame.ghosts, &mut |l, s| {
            behind(true);
            Self::draw(gl, &self.mesh, &s.opaque, frame, viewport, l, HOVER);
            behind(false);
            Self::draw(gl, &self.mesh, &s.translucent, frame, viewport, l, HOVER);
            Self::draw(gl, &self.line, &s.lines, frame, viewport, l, HOVER);
            Self::draw(gl, &self.point, &s.vertices, frame, viewport, l, HOVER);
        });
        each(&frame.scene, &mut |l, s| {
            Self::draw(gl, &self.mesh, &s.translucent, frame, viewport, l, HOVER);
        });
        unsafe { gl.disable(glow::DEPTH_TEST) };
        // Datum frames, over whatever is built around them.
        let programs = [&self.mesh, &self.line, &self.point];
        each(&frame.ghosts, &mut |l, s| {
            for (program, buffer) in programs.into_iter().zip(&s.front) {
                Self::draw(gl, program, buffer, frame, viewport, l, HOVER);
            }
        });
        each(&frame.scene, &mut |l, s| {
            for (program, buffer) in programs.into_iter().zip(&s.front) {
                Self::draw(gl, program, buffer, frame, viewport, l, HOVER);
            }
        });
        if let Some(h) = &self.handles {
            let l = &frame.handles;
            let programs = [&self.mesh, &self.line, &self.point];
            Self::draw(gl, &self.line, &h.lines, frame, viewport, l, HANDLE_HOVER);
            Self::draw(
                gl,
                &self.point,
                &h.vertices,
                frame,
                viewport,
                l,
                HANDLE_HOVER,
            );
            // What the handles' scene puts in front: a connector offered.
            for (program, buffer) in programs.into_iter().zip(&h.front) {
                Self::draw(gl, program, buffer, frame, viewport, l, HANDLE_HOVER);
            }
        }
        unsafe {
            gl.depth_mask(true);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }
}
