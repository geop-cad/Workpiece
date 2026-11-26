//! Solving an assembly: where its joints leave its instances.
//!
//! Every instance that is not fixed has seven numbers, where it is and
//! how it is turned (a quaternion, kept of length one by an equation of
//! its own). A joint is a few equations between the frames of its two
//! connectors, told in the first's frame: which of the second's
//! directions must stay across it, and which of its offsets must stay
//! nothing. The solver (`workpiece-solver`) moves everything as little as it
//! can from where it is, so parts stay on the side they are on; a drag
//! pulls one part towards the pointer, and the joints have the last say.
//!
//! Limits are held by solving again with the joint at its limit, where a
//! solve without it went past.

use std::collections::BTreeMap;
use workpiece_core::math::{angle_between, rotation};

use glam::DQuat;
use workpiece_core::math::Point3;
use workpiece_core::world::{Joint, JointKind, World};
use workpiece_core::{Axis, Pose};
use workpiece_solver::{Pull, Scalar, System};

/// A part pulled by the pointer: the point `local` of `instance` (in
/// the instance's own coordinates) should come to `target`.
#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    /// The instance.
    pub instance: String,
    /// The point held, in its coordinates.
    pub local: Point3,
    /// Where it should go, in the assembly's.
    pub target: Point3,
}

/// How a solve went.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Solved {
    /// Where every instance that moves is.
    pub poses: BTreeMap<String, Pose>,
    /// Whether every joint holds.
    pub converged: bool,
    /// The joints at a limit.
    pub at_limit: Vec<String>,
    /// The mechanisms that could not be held: between joints of more
    /// than two parts that move.
    pub unsolved: Vec<String>,
}

/// Three numbers of whatever the equations are written over.
#[derive(Clone, Copy)]
struct V<S>([S; 3]);

impl<S: Scalar> V<S> {
    fn constant(p: Point3) -> Self {
        V([S::constant(p.x), S::constant(p.y), S::constant(p.z)])
    }

    fn add(self, o: Self) -> Self {
        V(std::array::from_fn(|k| self.0[k] + o.0[k]))
    }

    fn sub(self, o: Self) -> Self {
        V(std::array::from_fn(|k| self.0[k] - o.0[k]))
    }

    fn scale(self, s: S) -> Self {
        V(self.0.map(|x| x * s))
    }

    fn dot(self, o: Self) -> S {
        self.0[0] * o.0[0] + self.0[1] * o.0[1] + self.0[2] * o.0[2]
    }

    fn cross(self, o: Self) -> Self {
        let (a, b) = (self.0, o.0);
        V([
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ])
    }
}

/// Where a body is: seven of the solver's numbers, or a pose that does
/// not move.
#[derive(Clone, Copy)]
enum Body {
    Free(usize),
    Held(Pose),
}

/// A frame on a body, as the equations see it: its origin and axes.
struct Frame<S> {
    o: V<S>,
    x: V<S>,
    y: V<S>,
    z: V<S>,
}

/// The frame `local` of a body whose seven numbers are `x` (or which is
/// held at a pose).
fn frame<S: Scalar>(body: Body, x: &[S], local: &Pose) -> Frame<S> {
    let (o, ax, ay, az) = (
        local.translation(),
        local.axis(Axis::X),
        local.axis(Axis::Y),
        local.axis(Axis::Z),
    );
    match body {
        Body::Held(pose) => Frame {
            o: V::constant(pose.transform_point(o)),
            x: V::constant(pose.transform_vector(ax)),
            y: V::constant(pose.transform_vector(ay)),
            z: V::constant(pose.transform_vector(az)),
        },
        Body::Free(_) => {
            let t = V([x[0], x[1], x[2]]);
            let u = V([x[3], x[4], x[5]]);
            let w = x[6];
            let two = S::constant(2.0);
            let norm = u.dot(u) + w * w;
            // Turned by the quaternion, whatever its length.
            let turn = |p: Point3| {
                let v = V::<S>::constant(p);
                let turned = v
                    .scale(w * w - u.dot(u))
                    .add(u.scale(two * u.dot(v)))
                    .add(u.cross(v).scale(two * w));
                V(turned.0.map(|c| c / norm))
            };
            Frame {
                o: t.add(turn(o)),
                x: turn(ax),
                y: turn(ay),
                z: turn(az),
            }
        }
    }
}

/// A joint held at a limit.
#[derive(Clone, Copy, PartialEq)]
enum Held {
    Angle(f64),
    Distance(f64),
}

enum Equation {
    /// A body's quaternion has length one.
    Unit(usize),
    /// A point of a body is where the three numbers after the bodies'
    /// are: what a drag pulls.
    Grab { body: usize, local: Point3 },
    Joint {
        joint: usize,
        a: Body,
        b: Body,
        held: Vec<Held>,
    },
    /// Two joints' travels go together: since the solve started, the
    /// second has gone `rate` times as far as the first.
    Couple {
        /// The two joints, each with its two bodies and how far it had
        /// turned and slid when the solve started.
        joints: [(usize, Body, Body, (f64, f64)); 2],
        /// Whether each one's turning counts (else its sliding).
        turns: (bool, bool),
        rate: f64,
        /// The free bodies among the four, in the order their numbers
        /// are given.
        bodies: Vec<usize>,
    },
}

/// How far a joint has turned since it was at the angle `from`, between
/// minus and plus half a turn.
fn turned<S: Scalar>(a: &Frame<S>, b: &Frame<S>, from: f64) -> S {
    let (s0, c0) = libm::sincos(from);
    let (c, s) = (b.x.dot(a.x), b.x.dot(a.y));
    (s * S::constant(c0) - c * S::constant(s0)).atan2(c * S::constant(c0) + s * S::constant(s0))
}

struct Assembly<'a> {
    joints: &'a [Joint],
    bodies: usize,
    /// Whether three more numbers follow the bodies': a point held by
    /// the pointer.
    grabbed: bool,
    equations: Vec<Equation>,
}

/// The angle and the distance of a joint: of the second frame against
/// the first, around and along the first's z.
fn travel<S: Scalar>(a: &Frame<S>, b: &Frame<S>) -> (S, S) {
    (b.x.dot(a.y).atan2(b.x.dot(a.x)), b.o.sub(a.o).dot(a.z))
}

impl System for Assembly<'_> {
    fn variables(&self) -> usize {
        self.bodies * 7 + if self.grabbed { 3 } else { 0 }
    }

    fn constraints(&self) -> usize {
        self.equations.len()
    }

    fn uses(&self, k: usize) -> Vec<usize> {
        let of = |body: &Body| match body {
            Body::Free(i) => (i * 7..i * 7 + 7).collect(),
            Body::Held(_) => Vec::new(),
        };
        match &self.equations[k] {
            Equation::Unit(i) => (i * 7..i * 7 + 7).collect(),
            Equation::Grab { body, .. } => {
                let mut vars: Vec<usize> = (body * 7..body * 7 + 7).collect();
                vars.extend(self.bodies * 7..self.bodies * 7 + 3);
                vars
            }
            Equation::Joint { a, b, .. } => {
                let mut vars: Vec<usize> = of(a);
                vars.extend(of(b));
                vars
            }
            Equation::Couple { bodies, .. } => {
                bodies.iter().flat_map(|i| i * 7..i * 7 + 7).collect()
            }
        }
    }

    fn residuals<S: Scalar>(&self, k: usize, x: &[S], out: &mut Vec<S>) {
        match &self.equations[k] {
            Equation::Unit(_) => {
                let q = &x[3..7];
                out.push(q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3] - S::constant(1.0));
            }
            Equation::Grab { body, local } => {
                let held = frame(Body::Free(*body), &x[..7], &Pose::from_translation(*local));
                out.extend(held.o.sub(V([x[7], x[8], x[9]])).0);
            }
            Equation::Couple {
                joints,
                turns,
                rate,
                bodies,
            } => {
                let of = |body: Body| match body {
                    Body::Free(i) => {
                        let at = bodies.iter().position(|b| *b == i).unwrap_or(0) * 7;
                        &x[at..at + 7]
                    }
                    Body::Held(_) => &x[..0],
                };
                let gone = |k: usize, turning: bool| {
                    let (joint, a, b, (angle, distance)) = joints[k];
                    let joint = &self.joints[joint];
                    let fa = frame(a, of(a), &joint.a.frame);
                    let fb = frame(b, of(b), &joint.b.frame);
                    if turning {
                        turned(&fa, &fb, angle)
                    } else {
                        fb.o.sub(fa.o).dot(fa.z) - S::constant(distance)
                    }
                };
                out.push(gone(1, turns.1) - gone(0, turns.0) * S::constant(*rate));
            }
            Equation::Joint { joint, a, b, held } => {
                let joint = &self.joints[*joint];
                let split = if matches!(a, Body::Free(_)) { 7 } else { 0 };
                let fa = frame(*a, &x[..split], &joint.a.frame);
                let fb = frame(*b, &x[split..], &joint.b.frame);
                let apart = fb.o.sub(fa.o);
                let together = |out: &mut Vec<S>| out.extend(apart.0);
                let along = |out: &mut Vec<S>| out.extend([fb.z.dot(fa.x), fb.z.dot(fa.y)]);
                let on_axis = |out: &mut Vec<S>| out.extend([apart.dot(fa.x), apart.dot(fa.y)]);
                let unturned = |out: &mut Vec<S>| out.push(fb.x.dot(fa.y));
                match joint.kind {
                    JointKind::Fastened => {
                        together(out);
                        along(out);
                        unturned(out);
                    }
                    JointKind::Revolute => {
                        together(out);
                        along(out);
                    }
                    JointKind::Slider => {
                        along(out);
                        unturned(out);
                        on_axis(out);
                    }
                    JointKind::Cylindrical => {
                        along(out);
                        on_axis(out);
                    }
                    JointKind::Planar => {
                        along(out);
                        out.push(apart.dot(fa.z));
                    }
                    JointKind::Ball => together(out),
                }
                for held in held {
                    match *held {
                        // The second's x at that angle from the first's.
                        Held::Angle(angle) => {
                            let (s, c) = libm::sincos(angle);
                            out.push(
                                fb.x.dot(fa.y) * S::constant(c) - fb.x.dot(fa.x) * S::constant(s),
                            );
                        }
                        Held::Distance(d) => out.push(apart.dot(fa.z) - S::constant(d)),
                    }
                }
            }
        }
    }
}

fn numbers(pose: &Pose) -> [f64; 7] {
    let (t, q) = (pose.translation(), pose.rotation());
    [t.x, t.y, t.z, q.x, q.y, q.z, q.w]
}

fn pose_of(x: &[f64]) -> Pose {
    Pose::from_rotation_translation(
        DQuat::from_xyzw(x[3], x[4], x[5], x[6]).normalize(),
        Point3::new(x[0], x[1], x[2]),
    )
}

/// The pose that brings the frame `b` of a part onto the frame `a` of
/// another, placed at `at`: where a joint's second part snaps to when
/// the joint is made.
pub fn snapped(at: &Pose, a: &Pose, b: &Pose) -> Pose {
    *at * *a * b.inverse()
}

/// The name a step's entry of the cache has.
fn entry(owner: &str, name: &str) -> String {
    format!("{owner}/{name}")
}

/// The numbers a pose is kept as in a part's cache: where, and how
/// turned.
pub fn kept(pose: &Pose) -> workpiece_core::Kept {
    vec![("pose".to_owned(), numbers(pose).to_vec())]
}

/// Where the instance `name` is according to the part's `cache`: where
/// it was put, dragged or left by its joints; where its part's own
/// origin is, if it never was.
pub fn placed(cache: &workpiece_core::Cache, name: &str) -> Pose {
    match cache.get(&entry(name, "pose")) {
        Some(pose) if pose.len() == 7 && pose.iter().all(|x| x.is_finite()) => pose_of(pose),
        _ => Pose::IDENTITY,
    }
}

/// Puts the instances of `world` where the part's `cache` has them and
/// its joints leave them, and writes that to the cache: what runs after
/// a part's last step.
///
/// A joint the cache does not know yet brings its second part to its
/// first before anything is solved, so that a joint just made closes the
/// way it was made, not the nearest way from wherever the part lay.
pub fn settle(world: &mut World, cache: &mut workpiece_core::Cache) {
    if world.instances.is_empty() {
        return;
    }
    for (name, instance) in &mut world.instances {
        instance.pose = placed(cache, name);
    }
    // The parts a joint that was made before holds already: another
    // joint just made (a second mate) does not snap them to itself but
    // is solved from where they are.
    let mut held: std::collections::BTreeSet<String> = world
        .joints
        .iter()
        .filter(|j| cache.get(&entry(&j.name, "made")).is_some())
        .flat_map(|j| [j.a.instance.clone(), j.b.instance.clone()])
        .flatten()
        .collect();
    for joint in &world.joints {
        if cache.get(&entry(&joint.name, "made")).is_some() {
            continue;
        }
        let at = |c: &workpiece_core::world::Connector| match &c.instance {
            Some(name) => world.instances.get(name).map(|i| (i.pose, i.fixed)),
            None => Some((Pose::IDENTITY, true)),
        };
        let (Some((pose_a, fixed_a)), Some((pose_b, fixed_b))) = (at(&joint.a), at(&joint.b))
        else {
            continue;
        };
        // The second comes to the first; the first, if the second is
        // held.
        let free = |c: &workpiece_core::world::Connector| {
            c.instance.as_ref().is_some_and(|n| !held.contains(n))
        };
        let (moved, pose) = if !fixed_b && free(&joint.b) {
            (
                &joint.b.instance,
                snapped(&pose_a, &joint.a.frame, &joint.b.frame),
            )
        } else if !fixed_a && free(&joint.a) {
            (
                &joint.a.instance,
                snapped(&pose_b, &joint.b.frame, &joint.a.frame),
            )
        } else {
            continue;
        };
        if let Some(instance) = moved.as_ref().and_then(|n| world.instances.get_mut(n)) {
            instance.pose = pose;
        }
        held.extend(moved.clone());
    }
    let solved = solve(world, None);
    for (name, pose) in solved.poses {
        if let Some(instance) = world.instances.get_mut(&name) {
            instance.pose = pose;
        }
    }
    keep(cache, world, &BTreeMap::new());
}

/// Writes to the part's `cache` where the instances of its `world` are
/// — those in `moved` where that has them (what a drag leaves), the
/// others where they are — and that its joints are made, as [`settle`]
/// leaves them: the part's next build starts from all of it. (A part
/// placed in the one that is open is built from its own cache, which
/// no build writes: a drag of something in it writes everything, or
/// the next build would snap what it never knew made.)
pub fn keep(cache: &mut workpiece_core::Cache, world: &World, moved: &BTreeMap<String, Pose>) {
    // Each entry by itself: the first mate of a placed part makes a
    // joint named as the step that places it, so that the joint's mark
    // and where the part is are entries of the same step.
    for (name, instance) in &world.instances {
        let pose = moved.get(name).unwrap_or(&instance.pose);
        // (A part at its own origin has nothing to remember.)
        let numbers = (*pose != Pose::IDENTITY).then(|| numbers(pose).to_vec());
        cache.set(&entry(name, "pose"), numbers);
    }
    for joint in &world.joints {
        cache.set(&entry(&joint.name, "made"), Some(vec![1.0]));
    }
}

/// Solves the joints of `world` from where its instances are, pulling
/// `drag` if there is one.
pub fn solve(world: &World, drag: Option<&Drag>) -> Solved {
    // The bodies: the instances that may move and that something acts on.
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    for joint in &world.joints {
        for name in [&joint.a.instance, &joint.b.instance] {
            if let Some(name) = name.as_deref()
                && world.instances.get(name).is_some_and(|i| !i.fixed)
            {
                let next = index.len();
                index.entry(name).or_insert(next);
            }
        }
    }
    if let Some(drag) = drag
        && world
            .instances
            .get(&drag.instance)
            .is_some_and(|i| !i.fixed)
    {
        let next = index.len();
        index.entry(drag.instance.as_str()).or_insert(next);
    }
    let mut solved = Solved {
        converged: true,
        ..Solved::default()
    };
    if index.is_empty() {
        return solved;
    }
    // A part that nothing holds follows the pointer as it is: moved,
    // never turned.
    let held = |name: &str| {
        world.joints.iter().any(|j| {
            [&j.a.instance, &j.b.instance]
                .into_iter()
                .any(|i| i.as_deref() == Some(name))
        })
    };
    if let Some(drag) = drag
        && index.len() == 1
        && index.contains_key(drag.instance.as_str())
        && !held(&drag.instance)
    {
        let pose = world.instances[&drag.instance].pose;
        let shift = drag.target - pose.transform_point(drag.local);
        solved
            .poses
            .insert(drag.instance.clone(), Pose::from_translation(shift) * pose);
        return solved;
    }
    let body = |name: &Option<String>| -> Option<Body> {
        Some(match name.as_deref() {
            None => Body::Held(Pose::IDENTITY),
            Some(name) => match index.get(name) {
                Some(&i) => Body::Free(i),
                None => Body::Held(world.instances.get(name)?.pose),
            },
        })
    };
    // The point a drag holds, after the bodies.
    let grab = drag.and_then(|d| Some((*index.get(d.instance.as_str())?, d)));
    let mut x = vec![0.0; index.len() * 7 + if grab.is_some() { 3 } else { 0 }];
    for (name, &i) in &index {
        x[i * 7..i * 7 + 7].copy_from_slice(&numbers(&world.instances[*name].pose));
    }
    if let Some((_, drag)) = grab {
        let at = world.instances[&drag.instance]
            .pose
            .transform_point(drag.local);
        x[index.len() * 7..].copy_from_slice(&at.to_array());
    }
    // A joint's two frames look the same way along its axis (and, where
    // it does not turn, across it). The equations hold as well with one
    // of them the other way around, and the solve ends on the side it
    // starts nearer to: a part that starts far from looking the right
    // way — its joint just turned to face the other way, or put onto
    // another face — is turned to look that way first. (Once for each
    // part, by the first joint that holds it.)
    let mut turned_over = vec![false; index.len()];
    for joint in &world.joints {
        if joint.kind == JointKind::Ball {
            continue;
        }
        let (Some(a), Some(b)) = (body(&joint.a.instance), body(&joint.b.instance)) else {
            continue;
        };
        let pose = |x: &[f64], body: Body| match body {
            Body::Held(pose) => pose,
            Body::Free(i) => {
                let n = &x[i * 7..i * 7 + 7];
                let q = glam::DQuat::from_xyzw(n[3], n[4], n[5], n[6]).normalize();
                Pose::from_rotation_translation(q, Point3::new(n[0], n[1], n[2]))
            }
        };
        // The part that is turned: the second if it may move, else the first.
        let (moved, i, second) = match (a, b) {
            (_, Body::Free(i)) => (b, i, true),
            (Body::Free(i), _) => (a, i, false),
            _ => continue,
        };
        if turned_over[i] {
            continue;
        }
        for across in [false, true] {
            if across && !matches!(joint.kind, JointKind::Fastened | JointKind::Slider) {
                continue;
            }
            let (fa, fb) = (pose(&x, a) * joint.a.frame, pose(&x, b) * joint.b.frame);
            let (ra, rb) = (fa.rotation(), fb.rotation());
            let (ours, theirs, about) = match across {
                false => (rb * Point3::Z, ra * Point3::Z, ra * Point3::X),
                true => (rb * Point3::X, ra * Point3::X, ra * Point3::Z),
            };
            // (Near enough already: the solve finds the rest.)
            if ours.dot(theirs) >= 0.9 {
                continue;
            }
            // The shortest turn that brings it to look the same way,
            // through where the part's own frame is; half a turn about
            // the joint, if it looks just the other way.
            let through = if second {
                fb.translation()
            } else {
                fa.translation()
            };
            let (from, to) = if second {
                (ours, theirs)
            } else {
                (theirs, ours)
            };
            let around = from.cross(to);
            let rotation = if around.length() < 1e-9 {
                rotation(about, std::f64::consts::PI)
            } else {
                rotation(around.normalize(), angle_between(from, to))
            };
            let turn = Pose::from_translation(through)
                * Pose::from_rotation(rotation)
                * Pose::from_translation(-through);
            let now = turn * pose(&x, moved);
            x[i * 7..i * 7 + 7].copy_from_slice(&numbers(&now));
            turned_over[i] = true;
        }
    }
    let start = x.clone();
    let scale = x[..index.len() * 7]
        .chunks(7)
        .flat_map(|c| &c[..3])
        .fold(1.0_f64, |m, v| m.max(v.abs()));
    let tolerance = 1e-9 * scale;

    let mut held: Vec<Vec<Held>> = vec![Vec::new(); world.joints.len()];
    // Without limits first; then with every joint that went past one
    // held at it, until none does.
    for _ in 0..4 {
        let mut equations: Vec<Equation> = (0..index.len()).map(Equation::Unit).collect();
        for (k, joint) in world.joints.iter().enumerate() {
            let (Some(a), Some(b)) = (body(&joint.a.instance), body(&joint.b.instance)) else {
                continue;
            };
            if matches!((a, b), (Body::Held(_), Body::Held(_))) {
                continue;
            }
            equations.push(Equation::Joint {
                joint: k,
                a,
                b,
                held: held[k].clone(),
            });
        }
        for coupling in &world.couplings {
            let find = |name: &str| {
                let k = world.joints.iter().position(|j| j.name == name)?;
                let joint = &world.joints[k];
                let (a, b) = (body(&joint.a.instance)?, body(&joint.b.instance)?);
                // Where the joint is as the solve starts.
                let of = |body: Body| match body {
                    Body::Free(i) => &start[i * 7..i * 7 + 7],
                    Body::Held(_) => &start[..0],
                };
                let at = travel(
                    &frame::<f64>(a, of(a), &joint.a.frame),
                    &frame::<f64>(b, of(b), &joint.b.frame),
                );
                Some((k, a, b, at))
            };
            let (Some(first), Some(second)) = (find(&coupling.a), find(&coupling.b)) else {
                continue;
            };
            let mut bodies: Vec<usize> = Vec::new();
            for body in [first.1, first.2, second.1, second.2] {
                if let Body::Free(i) = body
                    && !bodies.contains(&i)
                {
                    bodies.push(i);
                }
            }
            // (An equation is over two parts' numbers at most: the
            // joints of a mechanism share a part, or stand on fixed
            // ones.)
            if bodies.is_empty() || bodies.len() > 2 {
                solved.unsolved.push(coupling.name.clone());
                continue;
            }
            equations.push(Equation::Couple {
                joints: [first, second],
                turns: coupling.kind.turns(),
                rate: coupling.kind.rate(),
                bodies,
            });
        }
        if let Some((body, drag)) = grab {
            equations.push(Equation::Grab {
                body,
                local: drag.local,
            });
        }
        let system = Assembly {
            joints: &world.joints,
            bodies: index.len(),
            grabbed: grab.is_some(),
            equations,
        };
        x.copy_from_slice(&start);
        // A joint held at a place along its travel starts there: turned
        // (or slid) to it, which the solve, going only a little way,
        // would not find from far off.
        for (k, joint) in world.joints.iter().enumerate() {
            let (Some(a), Some(b)) = (body(&joint.a.instance), body(&joint.b.instance)) else {
                continue;
            };
            for held in &held[k] {
                let pose = |x: &[f64], body: Body| match body {
                    Body::Held(pose) => pose,
                    Body::Free(i) => {
                        let n = &x[i * 7..i * 7 + 7];
                        let q = glam::DQuat::from_xyzw(n[3], n[4], n[5], n[6]).normalize();
                        Pose::from_rotation_translation(q, Point3::new(n[0], n[1], n[2]))
                    }
                };
                let (fa, fb) = (pose(&x, a) * joint.a.frame, pose(&x, b) * joint.b.frame);
                let (ra, bx) = (fa.rotation(), fb.rotation() * Point3::X);
                let (ax, ay, az) = (ra * Point3::X, ra * Point3::Y, ra * Point3::Z);
                let (turn, shift) = match *held {
                    Held::Angle(to) => (to - libm::atan2(bx.dot(ay), bx.dot(ax)), 0.0),
                    Held::Distance(to) => (0.0, to - (fb.translation() - fa.translation()).dot(az)),
                };
                let through = fa.translation();
                let moved = Pose::from_translation(az * shift + through)
                    * Pose::from_rotation(rotation(az, turn))
                    * Pose::from_translation(-through);
                match (a, b) {
                    (_, Body::Free(i)) => {
                        let now = moved * pose(&x, b);
                        x[i * 7..i * 7 + 7].copy_from_slice(&numbers(&now));
                    }
                    (Body::Free(i), _) => {
                        let now = moved.inverse() * pose(&x, a);
                        x[i * 7..i * 7 + 7].copy_from_slice(&numbers(&now));
                    }
                    _ => {}
                }
            }
        }
        let mut pull = Pull::none(&x);
        if let Some((body, drag)) = grab {
            // A part rather shifts than turns: one that nothing holds
            // follows the pointer as it is, one on a hinge turns
            // because it has to.
            for k in 3..7 {
                pull.weight[body * 7 + k] = 1e6;
            }
            // The held point is pulled to the pointer, much harder than
            // anything is held where it is; the part comes with it as
            // far as the joints let it.
            for (k, v) in drag.target.to_array().into_iter().enumerate() {
                pull.target[index.len() * 7 + k] = v;
                pull.weight[index.len() * 7 + k] = 1e9;
            }
        }
        let report = workpiece_solver::solve(&system, &mut x, &pull, tolerance);
        solved.converged = report.converged;
        // Which joints went past a limit.
        let mut more = false;
        for (k, joint) in world.joints.iter().enumerate() {
            let (Some(a), Some(b)) = (body(&joint.a.instance), body(&joint.b.instance)) else {
                continue;
            };
            let of = |body: Body| match body {
                Body::Free(i) => &x[i * 7..i * 7 + 7],
                Body::Held(_) => &x[..0],
            };
            let (fa, fb) = (
                frame::<f64>(a, of(a), &joint.a.frame),
                frame::<f64>(b, of(b), &joint.b.frame),
            );
            let (angle, distance) = travel(&fa, &fb);
            if let Some((lo, hi)) = joint.limits.angle
                && joint.kind.turns()
                && !held[k].iter().any(|h| matches!(h, Held::Angle(_)))
                && (angle < lo - 1e-9 || angle > hi + 1e-9)
            {
                held[k].push(Held::Angle(angle.clamp(lo, hi)));
                more = true;
            }
            if let Some((lo, hi)) = joint.limits.distance
                && joint.kind.slides()
                && !held[k].iter().any(|h| matches!(h, Held::Distance(_)))
                && (distance < lo - tolerance || distance > hi + tolerance)
            {
                held[k].push(Held::Distance(distance.clamp(lo, hi)));
                more = true;
            }
        }
        if !more {
            break;
        }
    }
    solved.unsolved.sort();
    solved.unsolved.dedup();
    solved.at_limit = world
        .joints
        .iter()
        .zip(&held)
        .filter(|(_, h)| !h.is_empty())
        .map(|(j, _)| j.name.clone())
        .collect();
    for (name, &i) in &index {
        solved
            .poses
            .insert((*name).to_owned(), pose_of(&x[i * 7..i * 7 + 7]));
    }
    solved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use workpiece_core::math::{rotation_x, rotation_y, rotation_z};
    use workpiece_core::world::{Connector, Instance, Limits};

    fn world(parts: &[(&str, Pose, bool)]) -> World {
        let mut world = World::new();
        let part = Arc::new(World::new());
        for (name, pose, fixed) in parts {
            world.instances.insert(
                (*name).into(),
                Instance {
                    pose: *pose,
                    world: part.clone(),
                    fixed: *fixed,
                },
            );
        }
        world
    }

    fn joint(kind: JointKind, a: (&str, Pose), b: (&str, Pose)) -> Joint {
        Joint {
            name: "joint".into(),
            kind,
            a: Connector {
                instance: Some(a.0.into()),
                frame: a.1,
            },
            b: Connector {
                instance: Some(b.0.into()),
                frame: b.1,
            },
            limits: Limits::default(),
        }
    }

    fn at(x: f64, y: f64, z: f64) -> Pose {
        Pose::from_translation(Point3::new(x, y, z))
    }

    #[test]
    fn a_joint_turned_to_face_the_other_way_turns_its_part_over() {
        // A peg whose top (z up, 3 above its foot) is joined to the top
        // of a base: looking the same way it hangs below; turned to face
        // it, it stands on it upside down, wherever it started.
        for kind in [JointKind::Revolute, JointKind::Fastened, JointKind::Planar] {
            let mut w = world(&[
                ("base", at(0.0, 0.0, 0.0), true),
                ("peg", at(0.0, 0.0, 0.0), false),
            ]);
            let over = Pose::from_rotation(rotation_x(std::f64::consts::PI));
            w.joints.push(joint(
                kind,
                ("base", at(0.0, 0.0, 1.0)),
                ("peg", at(0.0, 0.0, 3.0) * over),
            ));
            let solved = solve(&w, None);
            assert!(solved.converged, "{kind:?}");
            let peg = solved.poses["peg"];
            // The peg's own up looks down, and its foot is 3 above the base's top.
            assert!((peg.rotation() * Point3::Z).z < -0.999, "{kind:?}");
            assert!(
                (peg.transform_point(Point3::ZERO).z - 4.0).abs() < 1e-6,
                "{kind:?}"
            );
            // Put onto the base's side instead (its normal along x, at
            // x = 2), from where that left it: it sticks out of it.
            w.instances.get_mut("peg").expect("the peg").pose = peg;
            w.joints[0].a.frame = Pose::from_rotation_translation(
                rotation_y(std::f64::consts::FRAC_PI_2),
                Point3::new(2.0, 0.0, 0.0),
            );
            let solved = solve(&w, None);
            assert!(solved.converged, "{kind:?}");
            let foot = solved.poses["peg"].transform_point(Point3::ZERO);
            assert!((foot.x - 5.0).abs() < 1e-6, "{kind:?}: {foot:?}");
        }
    }

    #[test]
    fn a_fastened_part_follows_the_fixed_one() {
        let mut w = world(&[
            ("base", at(1.0, 2.0, 3.0), true),
            ("lid", at(9.0, 0.0, 0.0), false),
        ]);
        w.joints.push(joint(
            JointKind::Fastened,
            ("base", at(0.0, 0.0, 5.0)),
            ("lid", at(0.0, 0.0, -1.0)),
        ));
        let solved = solve(&w, None);
        assert!(solved.converged);
        let lid = solved.poses["lid"];
        assert!(
            lid.translation()
                .abs_diff_eq(Point3::new(1.0, 2.0, 9.0), 1e-7),
            "{lid:?}"
        );
        assert!(!solved.poses.contains_key("base"));
    }

    #[test]
    fn a_revolute_part_turns_when_dragged_and_stops_at_its_limit() {
        // An arm hinged at the origin around z, its tip at (10, 0, 0).
        let mut w = world(&[
            ("base", Pose::IDENTITY, true),
            ("arm", Pose::IDENTITY, false),
        ]);
        w.joints.push(joint(
            JointKind::Revolute,
            ("base", Pose::IDENTITY),
            ("arm", Pose::IDENTITY),
        ));
        let tip = Point3::new(10.0, 0.0, 0.0);
        let drag = |w: &World, target: Point3| {
            solve(
                w,
                Some(&Drag {
                    instance: "arm".into(),
                    local: tip,
                    target,
                }),
            )
        };
        // Pulled up along y, a few times as a pointer would.
        for _ in 0..40 {
            let solved = drag(&w, Point3::new(0.0, 10.0, 0.0));
            assert!(solved.converged);
            w.instances.get_mut("arm").expect("arm").pose = solved.poses["arm"];
        }
        let arm = w.instances["arm"].pose;
        // The hinge stays, and the tip is where it was pulled to.
        assert!(arm.translation().length() < 1e-6, "{arm:?}");
        let reached = arm.transform_point(tip);
        assert!(
            reached.abs_diff_eq(Point3::new(0.0, 10.0, 0.0), 1e-3),
            "{reached}"
        );

        // With a limit of 30 degrees it stops there.
        let limit = 30.0_f64.to_radians();
        w.instances.get_mut("arm").expect("arm").pose = Pose::IDENTITY;
        w.joints[0].limits.angle = Some((-limit, limit));
        for _ in 0..40 {
            let solved = drag(&w, Point3::new(0.0, 10.0, 0.0));
            w.instances.get_mut("arm").expect("arm").pose = solved.poses["arm"];
        }
        let reached = w.instances["arm"].pose.transform_point(tip);
        let angle = libm::atan2(reached.y, reached.x);
        assert!((angle - limit).abs() < 1e-6, "{}", angle.to_degrees());
    }

    #[test]
    fn a_joint_held_at_an_angle_turns_there_without_a_drag() {
        // A hinge driven to 90 degrees, from where it was put (0) or half
        // a turn round: settled, it is at 90.
        for start in [0.0_f64, 180.0] {
            let mut w = world(&[
                ("base", Pose::IDENTITY, true),
                (
                    "arm",
                    Pose::from_rotation(rotation_z(start.to_radians())),
                    false,
                ),
            ]);
            w.joints.push(joint(
                JointKind::Revolute,
                ("base", Pose::IDENTITY),
                ("arm", Pose::IDENTITY),
            ));
            let at = 90.0_f64.to_radians();
            w.joints[0].limits.angle = Some((at, at));
            let solved = solve(&w, None);
            let tip = solved.poses["arm"].transform_point(Point3::X);
            assert!(solved.converged, "{start}");
            assert!(tip.abs_diff_eq(Point3::Y, 1e-6), "{start}: {tip}");
        }
    }

    #[test]
    fn a_slider_keeps_its_direction_and_its_limits() {
        let mut w = world(&[
            ("rail", Pose::IDENTITY, true),
            ("cart", at(0.0, 0.0, 1.0), false),
        ]);
        let mut slider = joint(
            JointKind::Slider,
            ("rail", Pose::IDENTITY),
            ("cart", Pose::IDENTITY),
        );
        slider.limits.distance = Some((0.0, 5.0));
        w.joints.push(slider);
        let mut pose = w.instances["cart"].pose;
        for _ in 0..40 {
            let solved = solve(
                &w,
                Some(&Drag {
                    instance: "cart".into(),
                    local: Point3::ZERO,
                    target: Point3::new(3.0, 4.0, 20.0),
                }),
            );
            pose = solved.poses["cart"];
            w.instances.get_mut("cart").expect("cart").pose = pose;
        }
        // On the axis, at the far limit, not turned.
        assert!(
            pose.translation()
                .abs_diff_eq(Point3::new(0.0, 0.0, 5.0), 1e-6),
            "{pose:?}"
        );
        assert!(pose.axis(Axis::X).abs_diff_eq(Point3::X, 1e-6));
    }

    #[test]
    fn gears_turn_together_at_their_ratio() {
        use workpiece_core::world::{Coupling, CouplingKind};
        // Two wheels on a fixed base, each on its own axis along z.
        let mut w = world(&[
            ("base", Pose::IDENTITY, true),
            ("big", Pose::IDENTITY, false),
            ("small", at(3.0, 0.0, 0.0), false),
        ]);
        let mut on = |name: &str, x: f64| {
            let mut j = joint(
                JointKind::Revolute,
                ("base", at(x, 0.0, 0.0)),
                (name, Pose::IDENTITY),
            );
            j.name = name.into();
            w.joints.push(j);
        };
        on("big", 0.0);
        on("small", 3.0);
        w.couplings.push(Coupling {
            name: "mesh".into(),
            kind: CouplingKind::Gear { ratio: -2.0 },
            a: "big".into(),
            b: "small".into(),
        });
        // The big wheel is pulled round by a point on its rim.
        let rim = Point3::new(2.0, 0.0, 0.0);
        let mut target = 0.0_f64;
        for _ in 0..60 {
            target += 0.02;
            let solved = solve(
                &w,
                Some(&Drag {
                    instance: "big".into(),
                    local: rim,
                    target: Point3::new(2.0 * libm::cos(target), 2.0 * libm::sin(target), 0.0),
                }),
            );
            assert!(solved.converged && solved.unsolved.is_empty());
            for (name, pose) in solved.poses {
                w.instances.get_mut(&name).expect("a wheel").pose = pose;
            }
        }
        let angle = |name: &str| {
            let x = w.instances[name].pose.axis(Axis::X);
            libm::atan2(x.y, x.x)
        };
        assert!((angle("big") - 1.2).abs() < 1e-3, "{}", angle("big"));
        // Twice as far, the other way: 2.4 radians back.
        assert!((angle("small") + 2.4).abs() < 2e-3, "{}", angle("small"));
        // Both stayed on their axes.
        assert!(
            w.instances["small"]
                .pose
                .translation()
                .abs_diff_eq(Point3::new(3.0, 0.0, 0.0), 1e-6)
        );
    }

    #[test]
    fn a_screw_slides_as_it_turns() {
        use workpiece_core::world::{Coupling, CouplingKind};
        let mut w = world(&[
            ("nut", Pose::IDENTITY, true),
            ("bolt", Pose::IDENTITY, false),
        ]);
        let mut thread = joint(
            JointKind::Cylindrical,
            ("nut", Pose::IDENTITY),
            ("bolt", Pose::IDENTITY),
        );
        thread.name = "thread".into();
        w.joints.push(thread);
        w.couplings.push(Coupling {
            name: "screw".into(),
            kind: CouplingKind::Screw { lead: 2.0 },
            a: "thread".into(),
            b: "thread".into(),
        });
        // Pulled up along its axis by a point off the axis: it has to
        // turn to rise.
        let mut height = 0.0;
        for _ in 0..50 {
            height += 0.01;
            let solved = solve(
                &w,
                Some(&Drag {
                    instance: "bolt".into(),
                    local: Point3::ZERO,
                    target: Point3::new(0.0, 0.0, height),
                }),
            );
            assert!(solved.converged, "at {height}");
            w.instances.get_mut("bolt").expect("bolt").pose = solved.poses["bolt"];
        }
        let pose = w.instances["bolt"].pose;
        let x = pose.axis(Axis::X);
        let turned = libm::atan2(x.y, x.x);
        // A quarter of the lead up, a quarter turn round.
        assert!((pose.translation().z - 0.5).abs() < 1e-3, "{pose:?}");
        assert!(
            (turned - std::f64::consts::FRAC_PI_2).abs() < 5e-3,
            "{turned}"
        );
    }

    #[test]
    fn a_free_part_is_shifted_not_turned() {
        let w = world(&[("loose", at(1.0, 1.0, 1.0), false)]);
        let solved = solve(
            &w,
            Some(&Drag {
                instance: "loose".into(),
                local: Point3::new(3.0, 0.0, 0.0),
                target: Point3::new(4.0, 6.0, 1.0),
            }),
        );
        let pose = solved.poses["loose"];
        assert!(pose.axis(Axis::X).abs_diff_eq(Point3::X, 1e-3), "{pose:?}");
        assert!(
            pose.translation()
                .abs_diff_eq(Point3::new(1.0, 6.0, 1.0), 1e-2),
            "{pose:?}"
        );
    }

    #[test]
    fn a_free_part_goes_where_it_is_dragged() {
        let w = world(&[("loose", at(1.0, 1.0, 1.0), false)]);
        let solved = solve(
            &w,
            Some(&Drag {
                instance: "loose".into(),
                local: Point3::ZERO,
                target: Point3::new(4.0, 5.0, 6.0),
            }),
        );
        assert!(
            solved.poses["loose"]
                .translation()
                .abs_diff_eq(Point3::new(4.0, 5.0, 6.0), 1e-3)
        );
    }
}
