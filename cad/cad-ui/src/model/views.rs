//! What the model does about how parts are looked at: named camera
//! views, sections, the drawings made of them, and what parts say of
//! themselves.

use anyhow::Context;
use workpiece_core::math::{Axis, Point2, Point3};
use workpiece_core::world::{DatumPlane, ORIGIN};
use workpiece_core::{Reference, Source};
use workpiece_ops_drawing::sheet::{SheetView, Style, ViewKind};
use workpiece_ops_drawing::{Along, CameraView, Drawing, Looking, Section, SectionArgs, Sheet};
use workpiece_ops_sketch::Sketch;

use super::Model;
use crate::Message;

/// The sketch `id` on `plane`, of the lines from each of `points` (in
/// the plane) to the next.
fn line_sketch(id: &str, plane: Reference<DatumPlane>, points: &[Point2]) -> Sketch {
    let mut sketch = Sketch::new(id);
    sketch.args.plane = Some(plane);
    let ids: Vec<_> = points.iter().map(|p| sketch.args.add_point(*p)).collect();
    for pair in ids.windows(2) {
        sketch.args.add_line(pair[0], pair[1]);
    }
    sketch
}

impl Model {
    /// The camera view that is edited, if the step that is edited is
    /// one: the part it is of, and the step.
    pub fn camera_view(&self) -> Option<(String, CameraView)> {
        let part = self.focused_part()?;
        let step = self.editing.last()?;
        let op = self.workspace.part(&part).ok()?.operation(step)?;
        Some((part, op.downcast_ref::<CameraView>()?.clone()))
    }

    /// Whether the step that is edited is a camera view that looks as
    /// the view does, and the view is turned otherwise than it has it:
    /// it is to take the view once that rests.
    pub fn takes_view(&self) -> bool {
        self.camera_view().is_some_and(|(_, view)| {
            view.args.looking == Looking::Current && view.args.view != Some(self.stance())
        })
    }

    /// The camera as it is, or where it is flying to, as a step keeps
    /// it.
    pub(crate) fn stance(&self) -> workpiece_ops_appearance::View {
        let camera = self
            .viewport
            .transition
            .map_or(self.viewport.camera, |t| t.to);
        workpiece_ops_appearance::View {
            target: camera.target.to_array(),
            rotation: camera.rotation.to_array(),
            distance: camera.distance,
            fov_y: camera.fov_y,
            orthographic: camera.orthographic,
        }
    }

    /// Sends the camera to the named view `name` of what shows.
    pub(crate) fn look_from(&mut self, name: &str) -> anyhow::Result<()> {
        let world = &self.viewport.shown.world;
        let seen = *world
            .views
            .get(name)
            .with_context(|| format!("there is no camera view {name}"))?;
        let pose = self.viewport.shown.pose;
        let base = self
            .viewport
            .transition
            .map_or(self.viewport.camera, |t| t.to);
        let mut to = match seen.at {
            Some((target, distance)) => {
                let mut to = base.facing(
                    pose.transform_point(target).as_vec3(),
                    pose.transform_vector(seen.towards).as_vec3(),
                    pose.transform_vector(seen.up).as_vec3(),
                );
                to.distance = distance as f32;
                to
            }
            None => base.facing(
                base.target,
                pose.transform_vector(seen.towards).as_vec3(),
                pose.transform_vector(seen.up).as_vec3(),
            ),
        };
        to.orthographic = seen.orthographic;
        self.fly(to);
        Ok(())
    }

    /// Adds a drawing to the focused part and opens it: made from the
    /// sheet template `template`, or (`None`) an empty sheet like the
    /// first template's.
    pub(crate) fn new_drawing(&mut self, template: Option<&str>) -> anyhow::Result<()> {
        let part = self.focused_part().context("no active part")?;
        let sheets = &self.workspace.sheets;
        let from = match template {
            Some(name) => sheets
                .get(name)
                .with_context(|| format!("there is no sheet template {name}"))?
                .clone(),
            None => {
                let mut first = sheets.templates.first().cloned().unwrap_or_else(|| {
                    workpiece_ops_drawing::Template::three_views(
                        "ISO A3",
                        workpiece_ops_drawing::sheet::Format::A3,
                        false,
                    )
                });
                first.views.clear();
                first
            }
        };
        let program = self.workspace.part(&part)?;
        let id = program.fresh_id("drawing");
        let sheet = Sheet::of(&self.viewport.shown.world, program.unit, &id, &from);
        self.panel = None;
        self.update(Message::AddOperation {
            part,
            op: Drawing { id, args: sheet }.into(),
        })
    }

    /// The plane a sketch lies on that a view looking along `towards`
    /// looks square onto, and the plane itself: the plane the camera
    /// view `named` looks onto, if it looks onto one, or else the plane
    /// of the origin that lies so.
    fn plane_square_to(
        &self,
        part: &str,
        named: Option<&str>,
        towards: Point3,
    ) -> anyhow::Result<(Reference<DatumPlane>, DatumPlane)> {
        let world = &self.viewport.shown.world;
        let of_view = named
            .and_then(|id| self.workspace.part(part).ok()?.operation(id))
            .and_then(|op| op.downcast_ref::<CameraView>())
            .filter(|view| view.args.looking == Looking::Plane)
            .and_then(|view| view.args.plane.clone());
        let along = [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .map(|axis| Reference::<DatumPlane>::frame_axis(ORIGIN, axis));
        for plane in of_view.into_iter().chain(along) {
            if let Ok(found) = world.resolve(&plane)
                && found.normal().dot(towards).abs() > 1.0 - 1e-9
            {
                return Ok((plane, found));
            }
        }
        anyhow::bail!(
            "a section is drawn in a view that looks along an axis of the origin, or onto a plane (a camera view of the part): this view looks askew"
        )
    }

    /// `line` — corners in the own coordinates of the view `view` of
    /// `sheet` — as the points of a sketch on the plane that view looks
    /// onto: the plane, and the points in it, gone along so that what
    /// was to the left of the line in the view is to its left there.
    fn line_on_plane(
        &self,
        part: &str,
        sheet: &Sheet,
        view: &str,
        line: &[[f64; 2]],
    ) -> anyhow::Result<(Reference<DatumPlane>, Vec<Point2>)> {
        anyhow::ensure!(line.len() >= 2, "a section cuts along a line: two points");
        let (basis, middle) = self
            .with_subject(|subject| {
                workpiece_ops_drawing::lay::looking(sheet, subject, &self.sheets_kept, view)
            })
            .flatten()
            .with_context(|| format!("the sheet has no view {view}"))?;
        let named = match sheet.view(view).map(|v| &v.kind) {
            Some(ViewKind::Named { view }) => Some(view.as_str()),
            _ => None,
        };
        let [right, up, towards] = basis;
        let (reference, plane) = self.plane_square_to(part, named, towards)?;
        let mut points: Vec<Point2> = line
            .iter()
            .map(|p| {
                let d = middle + right * p[0] + up * p[1] - plane.origin();
                Point2::new(d.dot(plane.x_dir()), d.dot(plane.y_dir()))
            })
            .collect();
        // (Seen from its back, the plane is mirrored.)
        if plane.normal().dot(towards) < 0.0 {
            points.reverse();
        }
        Ok((reference, points))
    }

    /// Cuts the model open along `line`, drawn in the view `view` of
    /// the drawing that is edited (corners in that view's own
    /// coordinates): the part gets a sketch of the line, on the plane
    /// that view looks onto, and a section along it — both before the
    /// drawing — and the sheet a view of that section, at `at`.
    pub(crate) fn cut_section(
        &mut self,
        view: &str,
        line: &[[f64; 2]],
        at: [f64; 2],
    ) -> anyhow::Result<String> {
        let (part, drawing) = self.drawing().context("no drawing is edited")?;
        let (plane, points) = self.line_on_plane(&part, &drawing.args, view, line)?;
        let program = self.workspace.part(&part)?;
        let sketch = line_sketch(&program.fresh_id("cut"), plane, &points);
        let section = Section {
            id: program.fresh_id("section"),
            args: SectionArgs {
                along: Along::Sketch,
                sketch: Some(Reference::new(Source::Profile(sketch.id.clone()))),
                ..SectionArgs::default()
            },
        };
        let mut sheet = drawing.args.clone();
        let id = sheet.fresh_view("section");
        sheet.views.push(SheetView {
            id: id.clone(),
            kind: ViewKind::Section {
                of: Some(view.to_owned()),
                section: section.id.clone(),
                letter: sheet.fresh_letter(),
                turn: 0.0,
            },
            at,
            scale: None,
            style: Style::HiddenRemoved,
            bodies: Vec::new(),
            cut_only: false,
            half: None,
            section: None,
            caption_at: None,
        });
        self.change(None, |ws| {
            let program = &mut ws.part_mut(&part)?.program;
            let before = program
                .iter()
                .position(|op| op.id() == drawing.id)
                .context("the drawing is no step of the part")?;
            let slot = &mut program[before];
            *slot = Drawing {
                id: drawing.id.clone(),
                args: sheet,
            }
            .into();
            program.insert(before, section.into());
            program.insert(before, sketch.into());
            Ok(())
        })?;
        self.history.seal();
        Ok(id)
    }

    /// Makes the section the section view `view` of the drawing that is
    /// edited is of show the other side: its Section step takes away
    /// the other side.
    pub(crate) fn flip_section(&mut self, view: &str) -> anyhow::Result<()> {
        let (part, drawing) = self.drawing().context("no drawing is edited")?;
        let Some(ViewKind::Section { section, .. }) = drawing.args.view(view).map(|v| &v.kind)
        else {
            anyhow::bail!("{view} is no section view");
        };
        let section = section.clone();
        self.change(None, |ws| {
            let slot = ws
                .part_mut(&part)?
                .operation_mut(&section)
                .with_context(|| format!("the part has no section {section}"))?;
            let mut flipped = slot
                .downcast_ref::<Section>()
                .with_context(|| format!("{section} is no section"))?
                .clone();
            flipped.args.flip = !flipped.args.flip;
            *slot = flipped.into();
            Ok(())
        })?;
        self.history.seal();
        Ok(())
    }

    /// The sketch a section of the focused part cuts along, if it is
    /// one that only holds the line (no constraints): what dragging
    /// the line on a sheet draws anew.
    pub fn cut_sketch(&self, section: &str) -> Option<Sketch> {
        let part = self.workspace.part(&self.focused_part()?).ok()?;
        let section = part.operation(section)?.downcast_ref::<Section>()?;
        let name = section
            .args
            .sketch
            .as_ref()
            .filter(|_| section.args.along == Along::Sketch)?
            .name();
        let sketch = part.operation(name)?.downcast_ref::<Sketch>()?;
        sketch.args.constraints.is_empty().then(|| sketch.clone())
    }

    /// Makes the line the section view `view` of the drawing that is
    /// edited cuts along `line` (corners in the own coordinates of the
    /// view the line shows in): the part's sketch of it is drawn anew.
    pub(crate) fn move_section_line(
        &mut self,
        view: &str,
        line: &[[f64; 2]],
        settled: bool,
    ) -> anyhow::Result<()> {
        let (part, drawing) = self.drawing().context("no drawing is edited")?;
        let Some(ViewKind::Section {
            of: Some(of),
            section,
            ..
        }) = drawing.args.view(view).map(|v| &v.kind)
        else {
            anyhow::bail!("{view} is no section drawn in a view");
        };
        let was = self.cut_sketch(section).context(
            "the section cuts along a sketch with constraints of its own: edit the sketch",
        )?;
        let (plane, points) = self.line_on_plane(&part, &drawing.args, of, line)?;
        let sketch = line_sketch(&was.id, plane, &points);
        let key = (part.clone(), format!("cut {}", was.id));
        self.change(Some(key), |ws| {
            let part = ws.part_mut(&part)?;
            // (What the old line was solved to is not the new one's.)
            part.cache.replace(&sketch.id, Vec::new());
            let slot = part
                .operation_mut(&sketch.id)
                .context("the sketch is no step of the part")?;
            *slot = sketch.into();
            Ok(())
        })?;
        if settled {
            self.history.seal();
        }
        Ok(())
    }

    /// What the focused part says of itself, by property: its own
    /// values, what the properties are by default, and its name, its
    /// material and its mass.
    pub fn properties(&self) -> std::collections::BTreeMap<String, String> {
        let Some(path) = self.focused_part() else {
            return Default::default();
        };
        let Ok(part) = self.workspace.part(&path) else {
            return Default::default();
        };
        let mut all = self.workspace.schema.values(&part.properties);
        let name = all.entry("Name".to_owned()).or_default();
        if name.trim().is_empty() {
            *name = std::path::Path::new(&path)
                .file_stem()
                .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        }
        all.insert(
            "Material".to_owned(),
            part.material.clone().unwrap_or_default(),
        );
        // What it weighs, with what is placed in it, as every sheet
        // says it (kept by the world: measuring it takes long).
        let world = self.drawn();
        let densities: std::collections::BTreeMap<String, f64> = self
            .workspace
            .library
            .materials
            .iter()
            .map(|m| (m.name.clone(), m.density))
            .collect();
        use workpiece_ops_drawing::lay::{mass_text, solid_bodies, weight};
        let weighed = || weight(world, part.unit, &densities);
        let (kg, unweighed) = match world.stamp {
            0 => {
                let w = weighed();
                (w.kg, w.unweighed.len())
            }
            stamp => {
                let mut found = None;
                let kg = self.sheets_kept.number(format!("{stamp}/kg"), || {
                    let w = weighed();
                    found = Some(w.unweighed.len());
                    w.kg
                });
                let unweighed = self.sheets_kept.number(format!("{stamp}/unweighed"), || {
                    found.unwrap_or_else(|| weighed().unweighed.len()) as f64
                });
                (kg, unweighed as usize)
            }
        };
        all.insert(
            "Mass".to_owned(),
            mass_text(kg, unweighed, solid_bodies(world), part.unit),
        );
        all
    }
}
