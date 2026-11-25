//! Mesh files read as solids: an STL file's triangles as the flat faces
//! of the solid they bound.

use std::collections::HashMap;

use anyhow::Context;
use shapecraft_boolean::Solid;
use shapecraft_core::math::Point3;

/// More triangles than this are not made faces of: a mesh that fine is
/// a picture of a part, not a part to work on.
const MOST: usize = 20_000;

/// Whether `path` names a mesh file that is read as solids.
pub fn is_mesh(path: &str) -> bool {
    let lower = path.trim().to_ascii_lowercase();
    lower.ends_with(".stl") || lower.ends_with(".3mf")
}

/// The solids a mesh file at `path` holds (by what it is called: an STL
/// file's one, a 3MF file's objects), their faces called `{id}/…`.
pub fn import_mesh(bytes: &[u8], path: &str, id: &str) -> anyhow::Result<Vec<Solid>> {
    if path.trim().to_ascii_lowercase().ends_with(".3mf") {
        import_3mf(bytes, id)
    } else {
        Ok(vec![import_stl(bytes, id)?])
    }
}

/// The file called `name` (or, for a name that starts with a dot, the
/// first whose name ends so) in a zip file, unpacked.
fn unzipped(bytes: &[u8], name: &str) -> anyhow::Result<Vec<u8>> {
    let word = |at: usize, n: usize| -> anyhow::Result<usize> {
        let b = bytes.get(at..at + n).context("the zip file is cut short")?;
        Ok(b.iter().rev().fold(0, |all, &x| all << 8 | usize::from(x)))
    };
    // The directory is told of at the end.
    let end = (0..bytes.len().saturating_sub(21))
        .rev()
        .find(|&at| bytes[at..at + 4] == *b"PK\x05\x06")
        .context("not a zip file")?;
    let (count, mut at) = (word(end + 10, 2)?, word(end + 16, 4)?);
    for _ in 0..count {
        anyhow::ensure!(
            word(at, 4)? == 0x0201_4b50,
            "the zip file's directory is broken"
        );
        let (method, packed) = (word(at + 10, 2)?, word(at + 20, 4)?);
        let lengths = [word(at + 28, 2)?, word(at + 30, 2)?, word(at + 32, 2)?];
        let local = word(at + 42, 4)?;
        let called = bytes
            .get(at + 46..at + 46 + lengths[0])
            .context("the zip file is cut short")?;
        let called = String::from_utf8_lossy(called);
        let wanted = if name.starts_with('.') {
            called.to_ascii_lowercase().ends_with(name)
        } else {
            called.trim_start_matches('/') == name
        };
        if wanted {
            let from = local + 30 + word(local + 26, 2)? + word(local + 28, 2)?;
            let data = bytes
                .get(from..from + packed)
                .context("the zip file is cut short")?;
            return match method {
                0 => Ok(data.to_vec()),
                8 => miniz_oxide::inflate::decompress_to_vec(data)
                    .map_err(|e| anyhow::anyhow!("{called} cannot be unpacked: {e}")),
                other => anyhow::bail!("{called} is packed in a way ({other}) that is not read"),
            };
        }
        at += 46 + lengths.iter().sum::<usize>();
    }
    anyhow::bail!("there is no {name} in the file")
}

/// The value of attribute `name` of an XML tag's text.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        if before.is_none_or(char::is_whitespace) && after.starts_with('=') {
            let value = after[1..].trim_start();
            let quote = value.chars().next()?;
            let value = &value[1..];
            return Some(&value[..value.find(quote)?]);
        }
        rest = &rest[at + name.len()..];
    }
    None
}

/// The solids of a 3MF file: each object's mesh as the solid it bounds,
/// called `{id}` (the first) and `{id}/body2`, … An object that is put
/// together of others is not read.
pub fn import_3mf(bytes: &[u8], id: &str) -> anyhow::Result<Vec<Solid>> {
    let model = unzipped(bytes, ".model")?;
    let text = std::str::from_utf8(&model)
        .ok()
        .context("the model is not text")?;
    let mut solids = Vec::new();
    let mut points: Vec<Point3> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    for tag in text.split('<').skip(1) {
        let tag = tag.split('>').next().unwrap_or_default();
        let kind = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        // (Without the prefix a file may give the names.)
        let kind = kind.rsplit(':').next().unwrap_or_default();
        let number = |name: &str| -> anyhow::Result<f64> {
            let value = attribute(tag, name).with_context(|| format!("a {kind} without {name}"))?;
            value
                .trim()
                .parse()
                .ok()
                .with_context(|| format!("{value:?} is no number"))
        };
        match kind {
            "vertex" => points.push(Point3::new(number("x")?, number("y")?, number("z")?)),
            "triangle" => {
                let corner = |name: &str| -> anyhow::Result<usize> {
                    let k = number(name)? as usize;
                    anyhow::ensure!(k < points.len(), "a triangle of a vertex that is not there");
                    Ok(k)
                };
                faces.push([corner("v1")?, corner("v2")?, corner("v3")?]);
            }
            _ => {}
        }
        // An object ends: what it held is a solid.
        let closes = tag.trim().strip_prefix('/');
        if closes.is_some_and(|name| name.rsplit(':').next() == Some("object")) {
            if !faces.is_empty() {
                let called = match solids.len() {
                    0 => id.to_owned(),
                    n => format!("{id}/body{}", n + 1),
                };
                solids.push(faceted_of(&called, &points, &faces, true)?);
            }
            points.clear();
            faces.clear();
        }
    }
    anyhow::ensure!(!solids.is_empty(), "the file has no mesh");
    Ok(solids)
}

/// The solid `triangles` of `points` bound, with points at one place
/// made one. A mesh that does not close is mended first if `mend`, and
/// refused otherwise.
fn faceted_of(
    id: &str,
    points: &[Point3],
    triangles: &[[usize; 3]],
    mend: bool,
) -> anyhow::Result<Solid> {
    anyhow::ensure!(
        triangles.len() <= MOST,
        "the mesh has {} triangles: more than the {MOST} that are made faces of",
        triangles.len()
    );
    let mut index: HashMap<[u64; 3], usize> = HashMap::new();
    let mut merged = Vec::new();
    let mut faces = Vec::new();
    for triangle in triangles {
        let ids = triangle.map(|k| {
            let p = points[k];
            // (Zero is zero, whatever its sign.)
            let key = p.to_array().map(|x| (x + 0.0).to_bits());
            *index.entry(key).or_insert_with(|| {
                merged.push(p);
                merged.len() - 1
            })
        });
        if ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2] {
            faces.push(ids);
        }
    }
    // As it is, if it closes; else mended first.
    match shapecraft_boolean::faceted(id, &merged, &faces) {
        Ok(solid) => Ok(solid),
        Err(open) if !mend => Err(open.into()),
        Err(_) => Ok(shapecraft_boolean::faceted(
            id,
            &merged,
            &mended(&merged, &faces),
        )?),
    }
}

/// The solid the triangles of the tessellated solid or shell `#root` of
/// a STEP file bound (AP242: `TESSELLATED_SOLID`, `TESSELLATED_SHELL`,
/// of triangulated faces over lists of points): flat faces, those in one
/// plane made one. A solid's mesh is mended if it does not close; a
/// shell's has to close as it is, or it is a surface and no solid.
pub(crate) fn tessellated(r: &crate::geometry::Reader<'_>, root: u32) -> anyhow::Result<Solid> {
    use crate::part21::Value;
    use anyhow::Context;
    let instance = r.file.get(root).context("no such instance")?;
    let items = instance.own().list(1).context("no faces")?;
    let mut points: Vec<Point3> = Vec::new();
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    // Where each list of points starts among all of them.
    let mut lists: HashMap<u32, (usize, usize)> = HashMap::new();
    for item in items.values().filter_map(|v| v.reference()) {
        let face = r.file.get(item).context("no such face")?;
        // After the points, their count and the normals: the link to
        // the exact face (of a face, not of a set of triangles), the
        // points' numbers, and the triangles — or strips and fans.
        let (linked, complex) = match face.name() {
            "TRIANGULATED_FACE" => (1, false),
            "COMPLEX_TRIANGULATED_FACE" => (1, true),
            "TRIANGULATED_SURFACE_SET" => (0, false),
            "COMPLEX_TRIANGULATED_SURFACE_SET" => (0, true),
            other => anyhow::bail!("tessellated solid #{root} has a {other}: that is not read"),
        };
        let args = face.own();
        let list = args.reference(1).context("a face has no points")?;
        let (base, count) = match lists.get(&list) {
            Some(known) => *known,
            None => {
                let coordinates = r.file.get(list).context("no such list of points")?;
                let rows = coordinates.own().list(2).context("a list has no points")?;
                let base = points.len();
                for row in rows.values() {
                    let Value::List(row) = row else {
                        anyhow::bail!("a point of list #{list} is no list of numbers");
                    };
                    let at = |k: usize| row.real(k).context("a point has no three numbers");
                    points.push(Point3::new(at(0)?, at(1)?, at(2)?) * r.length);
                }
                lists.insert(list, (base, points.len() - base));
                (base, points.len() - base)
            }
        };
        // A corner is a number among the face's own points, if it has
        // such a list, and that a number of the list of points: from 1.
        let own: Vec<usize> = args
            .list(4 + linked)
            .map(|l| {
                l.values()
                    .filter_map(|v| v.real())
                    .map(|x| x as usize)
                    .collect()
            })
            .unwrap_or_default();
        let corner = |v: Value<'_>| -> anyhow::Result<usize> {
            let k = v.real().context("a corner is no number")? as usize;
            let k = if own.is_empty() {
                k
            } else {
                *own.get(k.wrapping_sub(1))
                    .context("a corner is not among its face's points")?
            };
            anyhow::ensure!(k >= 1 && k <= count, "a corner is not among the points");
            Ok(base + k - 1)
        };
        let rows = |index: usize| -> anyhow::Result<Vec<Vec<usize>>> {
            let Some(rows) = args.list(index) else {
                return Ok(Vec::new());
            };
            rows.values()
                .map(|row| match row {
                    Value::List(row) => row.values().map(&corner).collect(),
                    _ => anyhow::bail!("a triangle is no list of corners"),
                })
                .collect()
        };
        if !complex {
            for row in rows(5 + linked)? {
                let [a, b, c] = row[..] else {
                    anyhow::bail!("a triangle of {} corners", row.len());
                };
                triangles.push([a, b, c]);
            }
            continue;
        }
        // A strip: each corner after the second with the two before
        // it, turned the same way every time. A fan: with the first
        // and the one before.
        for strip in rows(5 + linked)? {
            for k in 2..strip.len() {
                let (a, b) = if k % 2 == 0 {
                    (strip[k - 2], strip[k - 1])
                } else {
                    (strip[k - 1], strip[k - 2])
                };
                triangles.push([a, b, strip[k]]);
            }
        }
        for fan in rows(6 + linked)? {
            for k in 2..fan.len() {
                triangles.push([fan[0], fan[k - 1], fan[k]]);
            }
        }
    }
    anyhow::ensure!(
        !triangles.is_empty(),
        "tessellated solid #{root} has no triangles"
    );
    let solid = faceted_of(
        "solid",
        &points,
        &triangles,
        instance.name() == "TESSELLATED_SOLID",
    )?;
    Ok(solid)
}

/// `triangles` mended as far as that is plain: those there twice once,
/// all turned to face one way across the edges they share, the holes
/// left open closed by triangles from one of their corners, and all of
/// it turned to face out of what it encloses.
pub fn mended(points: &[Point3], triangles: &[[usize; 3]]) -> Vec<[usize; 3]> {
    let mut faces: Vec<[usize; 3]> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for t in triangles {
        let mut key = *t;
        key.sort_unstable();
        if seen.insert(key) {
            faces.push(*t);
        }
    }
    let sides = |t: &[usize; 3]| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
    // Across each edge two triangles share, they walk it opposite ways:
    // from each triangle on to its neighbours, turning those that do
    // not.
    let mut beside: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (k, t) in faces.iter().enumerate() {
        for (a, b) in sides(t) {
            beside.entry((a.min(b), a.max(b))).or_default().push(k);
        }
    }
    let mut settled = vec![false; faces.len()];
    for start in 0..faces.len() {
        if settled[start] {
            continue;
        }
        settled[start] = true;
        let mut todo = vec![start];
        while let Some(k) = todo.pop() {
            for (a, b) in sides(&faces[k].clone()) {
                let Some(&[one, other]) = beside.get(&(a.min(b), a.max(b))).map(|v| &v[..]) else {
                    continue;
                };
                let next = if one == k { other } else { one };
                if settled[next] {
                    continue;
                }
                if sides(&faces[next]).contains(&(a, b)) {
                    faces[next].swap(1, 2);
                }
                settled[next] = true;
                todo.push(next);
            }
        }
    }
    // The edges of one triangle only are around holes: each hole is
    // walked the other way around, and closed from its first corner.
    let mut onward: HashMap<usize, usize> = HashMap::new();
    for t in &faces {
        for (a, b) in sides(t) {
            if beside
                .get(&(a.min(b), a.max(b)))
                .is_some_and(|v| v.len() == 1)
            {
                onward.insert(b, a);
            }
        }
    }
    while let Some(&first) = onward.keys().next() {
        let mut around = vec![first];
        let mut at = first;
        while let Some(next) = onward.remove(&at) {
            if next == first {
                break;
            }
            around.push(next);
            at = next;
        }
        for k in 1..around.len().saturating_sub(1) {
            faces.push([around[0], around[k], around[k + 1]]);
        }
    }
    // Facing out: what they enclose has more than nothing in it.
    let inside: f64 = faces
        .iter()
        .map(|t| points[t[0]].dot(points[t[1]].cross(points[t[2]])))
        .sum();
    if inside < 0.0 {
        for t in &mut faces {
            t.swap(1, 2);
        }
    }
    faces
}

/// The corners of the triangles of an STL file, binary or text.
fn triangles(bytes: &[u8]) -> anyhow::Result<Vec<[Point3; 3]>> {
    let count = bytes
        .get(80..84)
        .map(|n| u32::from_le_bytes([n[0], n[1], n[2], n[3]]) as usize);
    // A binary file is as long as its count says.
    if let Some(count) = count.filter(|n| bytes.len() == 84 + n * 50) {
        let number = |at: usize| {
            let b = &bytes[at..at + 4];
            f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        return Ok((0..count)
            .map(|k| {
                let at = 84 + k * 50 + 12;
                std::array::from_fn(|c| {
                    let at = at + c * 12;
                    Point3::new(number(at), number(at + 4), number(at + 8))
                })
            })
            .collect());
    }
    let text = std::str::from_utf8(bytes)
        .ok()
        .context("neither a binary nor a text STL file")?;
    let mut corners = Vec::new();
    let mut words = text.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("vertex") {
            let mut number = || -> anyhow::Result<f64> {
                let word = words.next().context("the file ends in a vertex")?;
                word.parse()
                    .ok()
                    .with_context(|| format!("{word:?} is no number"))
            };
            corners.push(Point3::new(number()?, number()?, number()?));
        }
    }
    anyhow::ensure!(
        !corners.is_empty() && corners.len() % 3 == 0,
        "neither a binary nor a text STL file"
    );
    Ok(corners.as_chunks::<3>().0.to_vec())
}

/// The solid an STL file's triangles bound, its faces called `{id}/f0`,
/// `{id}/f1`, …: flat, those in one plane made one.
pub fn import_stl(bytes: &[u8], id: &str) -> anyhow::Result<Solid> {
    let found = triangles(bytes)?;
    let points: Vec<Point3> = found.iter().flatten().copied().collect();
    let faces: Vec<[usize; 3]> = (0..found.len())
        .map(|k| [3 * k, 3 * k + 1, 3 * k + 2])
        .collect();
    faceted_of(id, &points, &faces, true)
}
