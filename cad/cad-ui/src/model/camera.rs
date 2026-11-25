//! The viewport's camera.

use glam::{Mat3, Mat4, Quat, Vec2, Vec3};
use workpiece_core::{Projection, Ray};

/// Pick tolerance in pixels.
pub const PICK_RADIUS_PX: f32 = 6.0;

/// The world's up direction: z up, with x forward (towards the viewer of
/// the front view) and y to the right.
pub const UP: Vec3 = Vec3::Z;

/// An orbit camera around `target` that turns freely like a trackball, as
/// in most CAD programs: no direction stays up, and moving the pointer in
/// circles rolls the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// What the camera looks at and orbits.
    pub target: Vec3,
    /// Turns the camera's axes (x right, y up, z towards the eye) into the
    /// world's.
    pub rotation: Quat,
    /// From the target to the eye.
    pub distance: f32,
    /// Vertical field of view, radians. Orthographic views show what a
    /// perspective view shows at the target.
    pub fov_y: f32,
    /// Parallel projection instead of perspective.
    pub orthographic: bool,
}

impl Default for Camera {
    fn default() -> Self {
        // From the front (+x) right (+y), above.
        let back = Vec3::new(1.3, 1.0, 0.9);
        Self {
            target: Vec3::ZERO,
            rotation: orientation(back, UP),
            distance: 150.0,
            fov_y: 40f32.to_radians(),
            orthographic: true,
        }
    }
}

/// The rotation of a camera looking from `back` (towards the eye) with
/// `up`, or what is nearest to it, up on the screen.
fn orientation(back: Vec3, up: Vec3) -> Quat {
    let back = back.normalize_or(UP);
    let mut up = up - back * up.dot(back);
    if up.length_squared() < 1e-8 {
        up = back.any_orthonormal_vector();
    }
    let up = up.normalize();
    Quat::from_mat3(&Mat3::from_cols(up.cross(back), up, back)).normalize()
}

/// How long the camera takes to a new view, in seconds.
pub const TRANSITION_SECONDS: f32 = 0.25;

/// The field of view an orthographic camera counts as while it turns
/// into a perspective one (or back): a perspective camera this narrow,
/// far away, shows almost the same.
const NARROW: f32 = 0.02;

/// The camera on its way from one view to another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// Where it started.
    pub from: Camera,
    /// Where it is going.
    pub to: Camera,
    /// How far it is, from 0 to 1.
    pub progress: f32,
}

impl Transition {
    /// The camera as it is now: eased in and out.
    pub fn camera(&self) -> Camera {
        let t = self.progress.clamp(0.0, 1.0);
        Camera::blend(&self.from, &self.to, t * t * (3.0 - 2.0 * t))
    }
}

impl Camera {
    /// The camera `t` of the way (0 to 1) from `from` to `to`: the target
    /// moves straight, the view turns the short way, and the size of
    /// what is seen at the target changes evenly. Between perspective and
    /// orthographic, the field of view narrows (or widens) while the
    /// camera moves away (or in), so what is at the target keeps its
    /// size: a dolly zoom.
    pub fn blend(from: &Camera, to: &Camera, t: f32) -> Camera {
        if t >= 1.0 {
            return *to;
        }
        let height = (from.view_height().ln() * (1.0 - t) + to.view_height().ln() * t).exp();
        let half = |c: &Camera| {
            if c.orthographic {
                NARROW / 2.0
            } else {
                c.fov_y / 2.0
            }
        };
        let mut camera = Camera {
            target: from.target.lerp(to.target, t),
            rotation: from.rotation.slerp(to.rotation, t).normalize(),
            ..*to
        };
        if from.orthographic != to.orthographic || (!to.orthographic && from.fov_y != to.fov_y) {
            let tan = (half(from).tan().ln() * (1.0 - t) + half(to).tan().ln() * t).exp();
            camera.fov_y = 2.0 * tan.atan();
            camera.orthographic = false;
        }
        camera.distance = height / (2.0 * (camera.fov_y / 2.0).tan());
        camera
    }

    /// The same view (what is looked at, from where, and how much of it
    /// shows) with the projection of `like`: what a camera caught between
    /// two projections settles to.
    #[must_use]
    pub fn settled(&self, like: &Camera) -> Camera {
        Camera {
            fov_y: like.fov_y,
            orthographic: like.orthographic,
            distance: self.view_height() / (2.0 * (like.fov_y / 2.0).tan()),
            ..*self
        }
    }

    /// The same camera looking at `target` from direction `back`, with
    /// `up` (or what is nearest to it) up on the screen.
    #[must_use]
    pub fn facing(&self, target: Vec3, back: Vec3, up: Vec3) -> Camera {
        Camera {
            target,
            rotation: orientation(back, up),
            ..*self
        }
    }

    /// Where the camera is.
    pub fn eye(&self) -> Vec3 {
        self.target + self.distance * self.back()
    }

    /// The screen's right direction in the world.
    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    /// The screen's up direction in the world.
    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// Unit vector from the target towards the eye.
    pub fn back(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    /// Height of the view at the target, in world units.
    pub fn view_height(&self) -> f32 {
        2.0 * self.distance * (self.fov_y / 2.0).tan()
    }

    /// World to OpenGL clip space (depth −1..1) for a viewport of
    /// `aspect` (width over height).
    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye(), self.target, self.up());
        use glam::camera::rh::proj::opengl as proj;
        let aspect = aspect.max(1e-3);
        // Depth ranges no deeper than needed, for depth precision: lines
        // are drawn a fixed fraction of it in front of their faces.
        let proj = if self.orthographic {
            let (h, depth) = (self.view_height() / 2.0, self.distance * 10.0);
            proj::orthographic(-h * aspect, h * aspect, -h, h, -depth, depth)
        } else {
            proj::perspective(
                self.fov_y,
                aspect,
                self.distance * 0.05,
                self.distance * 20.0,
            )
        };
        proj * view
    }

    /// The pick ray through `position` in a viewport of `size` (pixels,
    /// origin top left), [`PICK_RADIUS_PX`] wide.
    pub fn ray(&self, position: Vec2, size: Vec2) -> Option<Ray> {
        let size = size.max(Vec2::ONE);
        let ndc = Vec2::new(
            2.0 * position.x / size.x - 1.0,
            1.0 - 2.0 * position.y / size.y,
        );
        let inverse = self.view_proj(size.x / size.y).inverse();
        let near = inverse.project_point3(ndc.extend(-1.0));
        let far = inverse.project_point3(ndc.extend(1.0));
        let pixel = PICK_RADIUS_PX / size.y;
        let (start, projection) = if self.orthographic {
            let radius = f64::from(pixel * self.view_height());
            (near, Projection::Orthographic { radius })
        } else {
            let slope = f64::from(pixel * 2.0 * (self.fov_y / 2.0).tan());
            (self.eye(), Projection::Perspective { slope })
        };
        Ray::new(start.as_dvec3(), (far - near).as_dvec3(), projection)
    }

    /// Orbits by a pointer movement of `delta` pixels, as if dragging the
    /// scene like a trackball.
    pub fn orbit(&mut self, delta: Vec2) {
        const SPEED: f32 = 0.007;
        let angle = delta.length() * SPEED;
        if angle <= 0.0 {
            return;
        }
        let axis = (self.up() * delta.x + self.right() * delta.y).normalize();
        self.rotation = (Quat::from_axis_angle(axis, -angle) * self.rotation).normalize();
    }

    /// Moves the view with the pointer by `delta` pixels, in a viewport
    /// `height` pixels tall.
    pub fn pan(&mut self, delta: Vec2, height: f32) {
        let pixel = self.view_height() / height.max(1.0);
        self.target += (self.up() * delta.y - self.right() * delta.x) * pixel;
    }

    /// Zooms by a scroll of `delta` (positive is closer).
    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (-delta * 0.0015).exp()).clamp(0.05, 10_000.0);
    }

    /// The size of a pixel in model units at `at`, in a viewport `height`
    /// pixels tall.
    pub fn pixel_size(&self, at: Vec3, height: f32) -> f32 {
        let height = height.max(1.0);
        if self.orthographic {
            self.view_height() / height
        } else {
            let depth = (at - self.eye())
                .dot(-self.back())
                .max(self.distance * 0.05);
            2.0 * depth * (self.fov_y / 2.0).tan() / height
        }
    }

    /// Back to the starting view, keeping the projection: from the
    /// starting side, onto all of what lies between `extent`'s corners
    /// if there is anything (with room around it), else onto the origin.
    pub fn home(&mut self, extent: Option<(Vec3, Vec3)>) {
        *self = Self {
            orthographic: self.orthographic,
            ..Self::default()
        };
        if let Some((lo, hi)) = extent {
            let across = (hi - lo).length();
            if across > 0.0 {
                self.target = (lo + hi) / 2.0;
                self.distance = across * 0.75 / (self.fov_y / 2.0).tan();
            }
        }
    }

    /// Looks at the target from direction `back`, keeping the distance:
    /// with z up, or for views along z the screen's up nearest to the
    /// current one.
    pub fn look_from(&mut self, back: Vec3) {
        let back = back.normalize_or(UP);
        let up = if back.cross(UP).length_squared() > 1e-6 {
            UP
        } else {
            self.up()
        };
        self.rotation = orientation(back, up);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_keep_the_size_of_what_is_at_the_target() {
        let from = Camera::default();
        let mut to = from;
        to.orthographic = false;
        to.look_from(Vec3::Z);
        let mut transition = Transition {
            from,
            to,
            progress: 0.0,
        };
        assert_eq!(transition.camera().rotation, from.rotation);
        for step in 0..=10 {
            transition.progress = step as f32 / 10.0;
            let camera = transition.camera();
            // The same height at the target all the way: a dolly zoom.
            assert!((camera.view_height() - from.view_height()).abs() < 1e-3);
            assert!(camera.distance.is_finite() && camera.distance > 0.0);
        }
        assert_eq!(transition.camera(), to);
        // Halfway, it looks from between the two directions.
        transition.progress = 0.5;
        let back = transition.camera().back();
        assert!(back.dot(from.back()) > 0.5 && back.dot(to.back()) > 0.5);
    }

    #[test]
    fn the_center_ray_hits_the_target() {
        for orthographic in [true, false] {
            let camera = Camera {
                orthographic,
                ..Camera::default()
            };
            let size = Vec2::new(800.0, 600.0);
            let ray = camera.ray(size / 2.0, size).expect("a ray");
            let t = ray.direction().dot(camera.target.as_dvec3() - ray.start());
            assert!(ray.at(t).distance(camera.target.as_dvec3()) < 1e-3);
        }
    }
}
