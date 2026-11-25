use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use shapecraft_cad_part::Library;
use shapecraft_core::Pose;
use shapecraft_core::world::topo::FaceId;
use shapecraft_core::world::visibility::shown_topology;
use shapecraft_core::world::{Look, Visibility, World};
use shapecraft_ops_appearance::Shot;
use shapecraft_render::{Scene, Surface, Triangle};

use crate::Model;

/// The picture that is edited, as it shows in place of the model: what
/// it is traced from and how.
pub struct Picture {
    /// The part the picture is a step of.
    pub part: String,
    /// The step: what the picture is called.
    pub id: String,
    /// The picture: its size and its light.
    pub shot: Shot,
    /// What is traced: the model as it shows, with what it is made of.
    pub scene: Arc<Scene>,
    /// A number that is another when the scene is.
    pub stamp: u64,
    /// Whether the picture is taken from where the view is now.
    pub taken: bool,
}

/// How a body without a material shows: a light grey plastic.
const PLAIN: [u8; 3] = [207, 210, 216];

/// The triangles of everything of `world` that shows, as `pose` places
/// it, with what each is made of.
struct Gather<'a> {
    library: &'a Library,
    triangles: Vec<Triangle>,
    surfaces: Vec<Surface>,
    known: BTreeMap<Look, u32>,
}

impl Gather<'_> {
    fn surface(&mut self, look: &Look) -> u32 {
        if let Some(index) = self.known.get(look) {
            return *index;
        }
        let material = look.material.as_ref().and_then(|m| self.library.get(m));
        let linear = |c: u8| (f32::from(c) / 255.0).powf(2.2);
        let colour = look
            .colour
            .or(material.map(|m| m.colour))
            .unwrap_or(PLAIN)
            .map(linear);
        let surface = match material {
            Some(m) => Surface {
                colour,
                roughness: m.roughness as f32,
                metallic: m.metallic as f32,
                transparency: m.transparency as f32,
                texture: m.texture.code(),
                scale: m.scale as f32,
            },
            None => Surface {
                colour,
                roughness: 0.5,
                metallic: 0.0,
                transparency: 0.0,
                texture: 0,
                scale: 1.0,
            },
        };
        let index = self.surfaces.len() as u32;
        self.surfaces.push(surface);
        self.known.insert(look.clone(), index);
        index
    }

    fn world(&mut self, world: &World, pose: Pose, visibility: &Visibility) {
        let topo = &world.topo;
        let mut body_of: BTreeMap<FaceId, &str> = BTreeMap::new();
        for (body, _) in topo.bodies().iter() {
            if let Some(name) = topo.name_of(body) {
                for face in topo.body_faces(body).unwrap_or_default() {
                    body_of.insert(face, name);
                }
            }
        }
        for face in shown_topology(topo, visibility).faces {
            let look = world.face_look(face, body_of.get(&face).copied());
            let surface = self.surface(&look);
            let Some(mesh) = shapecraft_cad_rasterize::tessellate::face(topo, face) else {
                continue;
            };
            for (positions, normals) in &mesh.triangles {
                self.triangles.push(Triangle {
                    positions: positions.map(|p| pose.transform_point(p).as_vec3()),
                    normals: normals.map(|n| pose.transform_vector(n).as_vec3()),
                    surface,
                });
            }
        }
        for (_, instance) in world.shown_instances(visibility) {
            let placed = Visibility::placed(&instance.world, &visibility.shades);
            self.world(&instance.world, pose * instance.pose, &placed);
        }
    }
}

/// What a picture of `world` is traced from: all of it that shows
/// under `visibility`, made of what `library` has.
pub fn scene(world: &World, visibility: &Visibility, library: &Library) -> Scene {
    let mut gather = Gather {
        library,
        triangles: Vec::new(),
        surfaces: Vec::new(),
        known: BTreeMap::new(),
    };
    gather.world(world, Pose::IDENTITY, visibility);
    Scene::new(gather.triangles, gather.surfaces)
}

/// The light of `shot`, as the one that traces takes it.
pub fn lighting(shot: &Shot) -> shapecraft_render::Lighting {
    use shapecraft_ops_appearance::Sky;
    let linear = |c: u8| (f32::from(c) / 255.0).powf(2.2);
    shapecraft_render::Lighting {
        sky: match shot.sky {
            Sky::Studio => shapecraft_render::Sky::Studio,
            Sky::Outdoor => shapecraft_render::Sky::Outdoor,
            Sky::Dark => shapecraft_render::Sky::Dark,
        },
        sun: [shot.sun_turn as f32, shot.sun_height as f32],
        strength: 2f32.powf(shot.exposure as f32),
        ground: shot.ground,
        backdrop: shot.backdrop().map(linear),
    }
}

/// The view's `camera`, as the one that traces `scene` takes it.
pub fn camera(camera: &crate::model::Camera, scene: &Scene) -> shapecraft_render::Camera {
    // (A parallel view is taken from far enough off to have the whole
    // model before it.)
    let reach = scene.bounds().map_or(0.0, |(lo, hi)| {
        (hi - lo).length() + (camera.target - (lo + hi) / 2.0).length()
    });
    let (eye, height) = if camera.orthographic {
        let off = camera.distance.max(reach * 2.0);
        (camera.target + camera.back() * off, camera.view_height())
    } else {
        (camera.eye(), 2.0 * (camera.fov_y / 2.0).tan())
    };
    shapecraft_render::Camera {
        eye,
        right: camera.right(),
        up: camera.up(),
        back: camera.back(),
        height,
        orthographic: camera.orthographic,
    }
}

/// The camera a picture is taken with from `view`.
pub fn camera_of(view: &shapecraft_ops_appearance::View) -> crate::model::Camera {
    crate::model::Camera {
        target: glam::Vec3::from_array(view.target),
        rotation: glam::Quat::from_array(view.rotation).normalize(),
        distance: view.distance,
        fov_y: view.fov_y,
        orthographic: view.orthographic,
    }
}

impl Picture {
    /// The picture `model` is editing, if the step that is edited is
    /// one.
    pub fn new(model: &Model) -> Option<Self> {
        let (part, render) = model.render()?;
        let shown = &model.viewport.shown;
        let library = &model.workspace.library;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        shown.world.stamp.hash(&mut hasher);
        format!("{:?}{library:?}", shown.visibility).hash(&mut hasher);
        let stamp = hasher.finish();
        let mut kept = model
            .picture_kept
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let scene = match &*kept {
            // (A world no engine built has no stamp to know it by.)
            Some((of, scene)) if *of == stamp && shown.world.stamp != 0 => scene.clone(),
            _ => {
                let scene = Arc::new(scene(&shown.world, &shown.visibility, library));
                *kept = Some((stamp, scene.clone()));
                scene
            }
        };
        let camera = model.viewport.camera;
        let taken = render.args.view.is_some_and(|view| {
            view.target == camera.target.to_array()
                && view.rotation == camera.rotation.to_array()
                && view.distance == camera.distance
                && view.fov_y == camera.fov_y
                && view.orthographic == camera.orthographic
        });
        Some(Self {
            part,
            id: render.id,
            shot: render.args,
            scene,
            stamp,
            taken,
        })
    }
}
