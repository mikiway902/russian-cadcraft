//! Bounded polyhedral drawing geometry: closed orientable shells, orthographic hidden-line
//! removal and planar sections. Curved surfaces must be supplied as an explicit faceted mesh;
//! no unreported tessellation, solid-kernel claim or 2D substitute for a 3D section.
use crate::{Vec2, Vec3};
use std::{borrow::Cow, collections::BTreeMap};

const MAX_VERTICES: usize = 512;
const MAX_FACES: usize = 1024;
const MAX_WORK: usize = 4_000_000;
const EPS: f64 = 1e-8;

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh<'a> {
    pub vertices: Cow<'a, [Vec3]>,
    pub faces: Vec<Vec<usize>>,
}
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub right: Vec3,
    pub up: Vec3,
    pub toward_eye: Vec3,
}
#[derive(Clone, Copy, Debug)]
pub struct Plane {
    pub normal: Vec3,
    pub offset: f64,
}
#[derive(Clone, Copy, Debug)]
pub struct Stroke {
    pub body: usize,
    pub edge: [usize; 2],
    pub a: Vec2,
    pub b: Vec2,
    pub hidden: bool,
    pub section: bool,
}
#[derive(Clone, Debug)]
pub struct Projection {
    pub strokes: Vec<Stroke>,
    pub sections: Vec<Vec<Vec<Vec2>>>,
}
#[derive(Clone, Copy)]
struct Point {
    p: Vec2,
    z: f64,
}
impl Frame {
    pub fn new(toward_eye: Vec3, up: Vec3) -> Result<Self, String> {
        if !toward_eye.is_finite()
            || !up.is_finite()
            || !toward_eye.len().is_finite()
            || !up.len().is_finite()
            || toward_eye.len() < EPS
            || up.len() < EPS
        {
            return Err("nonzero finite view vectors required".into());
        }
        let n = toward_eye.normalized();
        let r = up.normalized().cross(n);
        if r.len() < EPS {
            return Err("view direction and up vector are parallel".into());
        }
        let r = r.normalized();
        Ok(Self { right: r, up: n.cross(r), toward_eye: n })
    }
    fn project(self, p: Vec3) -> Point {
        Point { p: Vec2::new(p.dot(self.right), p.dot(self.up)), z: p.dot(self.toward_eye) }
    }
    pub fn point(self, p: Vec3) -> Vec2 {
        self.project(p).p
    }
}
impl Plane {
    pub fn new(normal: Vec3, offset: f64) -> Result<Self, String> {
        let length = normal.len();
        if !normal.is_finite() || !length.is_finite() || length < EPS || !offset.is_finite() || offset.abs() > 1e9 {
            return Err("invalid cutting plane".into());
        }
        let offset = offset / length;
        if !offset.is_finite() || offset.abs() > 1e9 {
            return Err("normalized plane offset exceeds +/-1e9".into());
        }
        Ok(Self { normal: normal * (1.0 / length), offset })
    }
    fn distance(self, p: Vec3) -> f64 {
        self.normal.dot(p) - self.offset
    }
}
fn valid(p: Vec3) -> bool {
    p.is_finite() && p.x.abs() <= 1e9 && p.y.abs() <= 1e9 && p.z.abs() <= 1e9
}
fn edge_crosses_face(vertices: &[Vec3], ids: [usize; 2], face: &[usize], origin: Vec3, normal: Vec3) -> Result<bool, String> {
    let point = |id: usize| vertices.get(id).copied().ok_or_else(|| "face index out of range".to_string());
    let [ai, bi] = ids;
    let a = point(ai)?;
    let b = point(bi)?;
    let delta = b - a;
    let da = normal.dot(a - origin);
    let db = normal.dot(b - origin);
    if (da > EPS && db > EPS) || (da < -EPS && db < -EPS) {
        return Ok(false);
    }
    if da.abs() <= EPS && db.abs() <= EPS {
        if face.iter().zip(face.iter().cycle().skip(1)).take(face.len()).any(|(&a, &b)| (a == ai && b == bi) || (a == bi && b == ai)) {
            return Ok(false);
        }
        let mut lo: f64 = 0.0;
        let mut hi: f64 = 1.0;
        for (&i, &j) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let p = point(i)?;
            let edge = point(j)? - p;
            let x = edge.cross(a - p).dot(normal) / edge.len();
            let slope = edge.cross(delta).dot(normal) / edge.len();
            if slope.abs() < EPS {
                if x < -EPS {
                    return Ok(false);
                }
            } else if slope > 0.0 {
                lo = lo.max(-x / slope)
            } else {
                hi = hi.min(-x / slope)
            }
            if lo > hi + EPS {
                return Ok(false);
            }
        }
        if (hi - lo) * delta.len() > EPS * 4.0 {
            return Ok(true);
        }
        let hit = a + delta * ((lo + hi) * 0.5);
        return Ok(!ids.iter().any(|&id| face.contains(&id) && point(id).is_ok_and(|p| (p - hit).len() <= EPS * 4.0)));
    }
    if (da - db).abs() < EPS {
        return Ok(false);
    }
    let t = da / (da - db);
    if !(-EPS..=1.0 + EPS).contains(&t) {
        return Ok(false);
    }
    let hit = a + delta * t;
    for (&i, &j) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
        let p = point(i)?;
        let edge = point(j)? - p;
        if edge.cross(hit - p).dot(normal) < -EPS * edge.len() {
            return Ok(false);
        }
    }
    Ok(!ids.iter().any(|&id| face.contains(&id) && point(id).is_ok_and(|p| (p - hit).len() <= EPS * 4.0)))
}

impl Mesh<'_> {
    pub fn validate(&self) -> Result<(), String> {
        if self.vertices.len() < 4 || self.vertices.len() > MAX_VERTICES || self.faces.len() < 4 || self.faces.len() > MAX_FACES {
            return Err("closed shell needs 4..512 vertices and 4..1024 faces".into());
        }
        if self.vertices.iter().any(|p| !valid(*p)) {
            return Err("vertices must be finite and within +/-1e9".into());
        }
        let mut edges = BTreeMap::<(usize, usize), (usize, i32)>::new();
        let mut volume = 0.0;
        let mut used = [false; MAX_VERTICES];
        for face in &self.faces {
            if face.len() < 3 || face.len() > 64 {
                return Err("faces need 3..64 ordered convex coplanar vertices".into());
            }
            let first = *face.first().ok_or("empty face")?;
            let a = *self.vertices.get(first).ok_or("face index out of range")?;
            let b = *self.vertices.get(*face.get(1).ok_or("missing face vertex")?).ok_or("face index out of range")?;
            let c = *self.vertices.get(*face.get(2).ok_or("missing face vertex")?).ok_or("face index out of range")?;
            let normal = (b - a).cross(c - a);
            let area = normal.len();
            if area < EPS {
                return Err("degenerate face or collinear leading vertices".into());
            }
            let normal = normal * (1.0 / area);
            for (i, &index) in face.iter().enumerate() {
                let p = *self.vertices.get(index).ok_or("face index out of range")?;
                let next = *face.get((i + 1) % face.len()).ok_or("missing face vertex")?;
                let q = *self.vertices.get(next).ok_or("face index out of range")?;
                if index == next || (q - p).len() < EPS {
                    return Err("zero-length face edge".into());
                }
                if face.iter().filter(|&&n| n == index).count() != 1 {
                    return Err("repeated face vertex".into());
                }
                if normal.dot(p - a).abs() > EPS * 1.0_f64.max((p - a).len()) {
                    return Err("nonplanar face".into());
                }
                for &other in face {
                    let v = *self.vertices.get(other).ok_or("face index out of range")?;
                    if (q - p).cross(v - p).dot(normal) < -EPS {
                        return Err("nonconvex face: triangulate it explicitly".into());
                    }
                }
                if let Some(v) = used.get_mut(index) {
                    *v = true;
                }
                let key = (index.min(next), index.max(next));
                let e = edges.entry(key).or_default();
                e.0 += 1;
                e.1 += if index < next { 1 } else { -1 };
            }
            let origin = *self.vertices.first().ok_or("empty mesh")?;
            for pair in face.get(1..).unwrap_or_default().windows(2) {
                if let [b, c] = pair {
                    let b = self.vertices.get(*b).ok_or("face index out of range")?;
                    let c = self.vertices.get(*c).ok_or("face index out of range")?;
                    volume += (a - origin).dot((*b - origin).cross(*c - origin)) / 6.0;
                }
            }
        }
        if used.iter().take(self.vertices.len()).any(|u| !*u) || edges.values().any(|&(n, w)| n != 2 || w != 0) {
            return Err("shell is not a closed consistently oriented two-manifold".into());
        }
        if !volume.is_finite() || volume.abs() < EPS {
            return Err("shell has zero or invalid signed volume".into());
        }
        self.validate_embedding()?;
        Ok(())
    }
    /// Edge pairing alone also accepts intersecting closed shells and pinched vertices.
    fn validate_embedding(&self) -> Result<(), String> {
        let mut fans = vec![Vec::<(usize, usize)>::new(); self.vertices.len()];
        let mut frames = Vec::with_capacity(self.faces.len());
        let point = |id: usize| self.vertices.get(id).copied().ok_or("face index out of range");
        for face in &self.faces {
            let a = point(*face.first().ok_or("face too short")?)?;
            let b = point(*face.get(1).ok_or("face too short")?)?;
            let c = point(*face.get(2).ok_or("face too short")?)?;
            let mut min = a;
            let mut max = a;
            for (i, &id) in face.iter().enumerate() {
                let p = point(id)?;
                min = Vec3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
                max = Vec3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
                let prior = *face.get((i + face.len() - 1) % face.len()).ok_or("face vertex missing")?;
                let next = *face.get((i + 1) % face.len()).ok_or("face vertex missing")?;
                fans.get_mut(id).ok_or("face vertex missing")?.push((prior, next));
            }
            frames.push((a, (b - a).cross(c - a).normalized(), min, max));
        }
        for fan in fans {
            let mut remaining = fan;
            let Some((first, mut next)) = remaining.pop() else { return Err("unused shell vertex".into()) };
            while next != first {
                let Some(i) = remaining.iter().position(|&(a, _)| a == next) else {
                    return Err("nonmanifold or inconsistently oriented vertex fan".into());
                };
                let (_, end) = remaining.swap_remove(i);
                next = end;
            }
            if !remaining.is_empty() {
                return Err("pinched shell vertex joins disconnected surface fans".into());
            }
        }
        let mut work = 0usize;
        for (index, face) in self.faces.iter().enumerate() {
            let (_, _, min, max) = *frames.get(index).ok_or("missing face frame")?;
            for (other, target) in self.faces.iter().enumerate().skip(index + 1) {
                let (origin, normal, bmin, bmax) = *frames.get(other).ok_or("missing face frame")?;
                if min.x > bmax.x + EPS
                    || min.y > bmax.y + EPS
                    || min.z > bmax.z + EPS
                    || max.x < bmin.x - EPS
                    || max.y < bmin.y - EPS
                    || max.z < bmin.z - EPS
                {
                    continue;
                }
                for (source, target, origin, normal) in [
                    (face, target, origin, normal),
                    (target, face, frames.get(index).ok_or("missing face frame")?.0, frames.get(index).ok_or("missing face frame")?.1),
                ] {
                    for (&a, &b) in source.iter().zip(source.iter().cycle().skip(1)).take(source.len()) {
                        work += target.len();
                        if work > MAX_WORK {
                            return Err("shell intersection pair-work limit exceeded".into());
                        }
                        if edge_crosses_face(&self.vertices, [a, b], target, origin, normal)? {
                            return Err("self-intersecting shell faces or non-topological surface contact".into());
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub fn cuboid(min: Vec3, max: Vec3) -> Result<Self, String> {
        if !valid(min) || !valid(max) || max.x - min.x < EPS || max.y - min.y < EPS || max.z - min.z < EPS {
            return Err("cuboid requires finite ordered minimum and maximum".into());
        }
        let vertices = vec![
            min,
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            max,
            Vec3::new(min.x, max.y, max.z),
        ];
        let mesh = Self {
            vertices: vertices.into(),
            faces: vec![vec![0, 3, 2, 1], vec![4, 5, 6, 7], vec![0, 1, 5, 4], vec![1, 2, 6, 5], vec![2, 3, 7, 6], vec![3, 0, 4, 7]],
        };
        mesh.validate()?;
        Ok(mesh)
    }
    pub fn extrude(profile: &[Vec2], z0: f64, z1: f64) -> Result<Self, String> {
        if profile.len() < 3 || profile.len() > 64 || !z0.is_finite() || !z1.is_finite() || z1 - z0 < EPS {
            return Err("extrusion needs a convex profile of 3..64 vertices and increasing Z levels".into());
        }
        let mut vertices = Vec::with_capacity(profile.len() * 2);
        for z in [z0, z1] {
            vertices.extend(profile.iter().map(|p| p.to3(z)));
        }
        let n = profile.len();
        let mut faces = vec![(0..n).rev().collect(), (n..2 * n).collect()];
        for i in 0..n {
            faces.push(vec![i, (i + 1) % n, (i + 1) % n + n, i + n]);
        }
        let mesh = Self { vertices: vertices.into(), faces };
        mesh.validate()?;
        Ok(mesh)
    }
}
fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}
fn clip_polygon(points: &[Vec3], plane: Plane) -> Vec<Vec3> {
    let mut out = Vec::new();
    for (&a, &b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        let da = plane.distance(a);
        let db = plane.distance(b);
        if da <= EPS {
            out.push(a);
        }
        if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    out
}
fn cut_loops(mesh: &Mesh, plane: Plane) -> Result<Vec<Vec<Vec3>>, String> {
    let mut distances = [0.0; MAX_VERTICES];
    let mut signs = (false, false);
    for (distance, vertex) in distances.iter_mut().zip(mesh.vertices.iter()) {
        *distance = plane.distance(*vertex);
        signs.0 |= *distance > EPS;
        signs.1 |= *distance < -EPS;
    }
    if !signs.0 || !signs.1 {
        return Ok(Vec::new());
    }
    let mut segments = Vec::new();
    for face in &mesh.faces {
        // A vertex is keyed by (id, id), an interior intersection by its undirected
        // mesh edge. Adjacent faces must join by topology, not rounded coordinates.
        let mut points = Vec::<(usize, usize)>::new();
        for (&ai, &bi) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
            let da = *distances.get(ai).ok_or("face index out of range")?;
            let db = *distances.get(bi).ok_or("face index out of range")?;
            let point = if da.abs() <= EPS {
                Some((ai, ai))
            } else if (da < -EPS && db > EPS) || (da > EPS && db < -EPS) {
                Some((ai.min(bi), ai.max(bi)))
            } else {
                None
            };
            if let Some(p) = point
                && !points.contains(&p)
            {
                points.push(p);
            }
        }
        match points.as_slice() {
            [a, b] => segments.push(((*a).min(*b), (*a).max(*b))),
            [] | [_] => {}
            _ => return Err("cutting plane coincides with a face; choose a transverse cut".into()),
        }
    }
    // An on-plane mesh edge is contributed by both incident faces.
    segments.sort_unstable();
    segments.dedup();
    let mut loops = Vec::new();
    while let Some((first, next)) = segments.pop() {
        let mut path = vec![first, next];
        let mut end = next;
        while end != first {
            let mut found = None;
            for (i, (a, b)) in segments.iter().enumerate() {
                if *a == end || *b == end {
                    if found.is_some() {
                        return Err("section branches at a nonregular point".into());
                    }
                    found = Some((i, if *a == end { *b } else { *a }));
                }
            }
            let (index, p) = found.ok_or("section contour is not closed")?;
            segments.swap_remove(index);
            path.push(p);
            end = p;
            if path.len() > MAX_FACES * 2 {
                return Err("section contour exceeds work limit".into());
            }
        }
        path.pop();
        if path.len() >= 3 {
            let points = path
                .into_iter()
                .map(|(ai, bi)| {
                    let a = *mesh.vertices.get(ai).ok_or("face index out of range")?;
                    if ai == bi {
                        return Ok(a);
                    }
                    let b = *mesh.vertices.get(bi).ok_or("face index out of range")?;
                    let da = *distances.get(ai).ok_or("face index out of range")?;
                    let db = *distances.get(bi).ok_or("face index out of range")?;
                    // Always interpolate from the same end of the mesh edge.
                    Ok(a + (b - a) * (da / (da - db)))
                })
                .collect::<Result<Vec<_>, String>>()?;
            loops.push(points);
        }
    }
    Ok(loops)
}
fn triangle_interval(a: Point, b: Point, t: [Point; 3], parameter_eps: f64) -> Option<(f64, f64)> {
    let u = t[1].p - t[0].p;
    let v = t[2].p - t[0].p;
    let det = cross(u, v);
    if det.abs() < EPS {
        return None;
    }
    let bary = |p: Point| {
        let q = p.p - t[0].p;
        let x = cross(q, v) / det;
        let y = cross(u, q) / det;
        [1.0 - x - y, x, y]
    };
    let wa = bary(a);
    let wb = bary(b);
    let mut lo: f64 = 0.0;
    let mut hi: f64 = 1.0;
    for i in 0..3 {
        let x = wa[i];
        let d = wb[i] - x;
        if d.abs() < EPS {
            if x < -EPS {
                return None;
            }
        } else if d > 0.0 {
            lo = lo.max(-x / d)
        } else {
            hi = hi.min(-x / d)
        }
    }
    let za = wa.iter().zip(t).map(|(w, p)| w * p.z).sum::<f64>() - a.z;
    let zb = wb.iter().zip(t).map(|(w, p)| w * p.z).sum::<f64>() - b.z;
    let delta = zb - za;
    if delta.abs() < EPS {
        if za <= EPS {
            return None;
        }
    } else if delta > 0.0 {
        lo = lo.max((EPS - za) / delta)
    } else {
        hi = hi.min((EPS - za) / delta)
    }
    lo = lo.max(0.0);
    hi = hi.min(1.0);
    (hi - lo > parameter_eps).then_some((lo, hi))
}
fn inside(p: Vec2, loops: &[Vec<Vec2>]) -> bool {
    let mut hit = false;
    for path in loops {
        for (&a, &b) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                hit = !hit;
            }
        }
    }
    hit
}
fn cap_intervals(a: Point, b: Point, loops: &[Vec<Vec2>], depth: f64, parameter_eps: f64, out: &mut Vec<(f64, f64)>) {
    let delta = b.p - a.p;
    let mut cuts = vec![0.0, 1.0];
    for path in loops {
        for (&p, &q) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
            let v = q - p;
            let det = cross(delta, v);
            if det.abs() < EPS {
                continue;
            }
            let t = cross(p - a.p, v) / det;
            let u = cross(p - a.p, delta) / det;
            if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
                cuts.push(t);
            }
        }
    }
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < parameter_eps);
    for pair in cuts.windows(2) {
        if let [lo, hi] = pair {
            let m = (lo + hi) * 0.5;
            if inside(a.p + delta * m, loops) && a.z + (b.z - a.z) * m < depth - EPS {
                out.push((*lo, *hi));
            }
        }
    }
}

fn merge_intervals(intervals: &mut Vec<(f64, f64)>, parameter_eps: f64) {
    intervals.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    intervals.dedup_by(|current, prior| {
        if current.0 <= prior.1 + parameter_eps {
            prior.1 = prior.1.max(current.1);
            true
        } else {
            false
        }
    });
}
/// Coincident visible contours take precedence over hidden lines (GOST 2.303).
fn visible_precedence(mut strokes: Vec<Stroke>, work: &mut usize) -> Result<Vec<Stroke>, String> {
    strokes.sort_unstable_by_key(|s| (s.hidden, !s.section, s.body, s.edge));
    let mut result = Vec::<Stroke>::with_capacity(strokes.len());
    for stroke in strokes {
        let delta = stroke.b - stroke.a;
        let length = delta.len();
        let mut covered = Vec::new();
        if length < EPS {
            continue;
        }
        let parameter_eps = EPS / length;
        for previous in &result {
            *work += 1;
            if *work > MAX_WORK {
                return Err("projection pair-work limit exceeded".into());
            }
            if cross(delta, previous.a - stroke.a).abs() > EPS * length || cross(delta, previous.b - stroke.a).abs() > EPS * length {
                continue;
            }
            let a = (previous.a - stroke.a).dot(delta) / (length * length);
            let b = (previous.b - stroke.a).dot(delta) / (length * length);
            let lo = a.min(b).max(0.0);
            let hi = a.max(b).min(1.0);
            if hi - lo > parameter_eps {
                covered.push((lo, hi));
            }
        }
        merge_intervals(&mut covered, parameter_eps);
        let mut last = 0.0;
        for (lo, hi) in covered {
            if lo - last > parameter_eps {
                result.push(Stroke { a: stroke.a + delta * last, b: stroke.a + delta * lo, ..stroke });
            }
            last = last.max(hi);
        }
        if 1.0 - last > parameter_eps {
            result.push(Stroke { a: stroke.a + delta * last, ..stroke });
        }
        if result.len() > 16384 {
            return Err("projection exceeds 16384 visible/hidden fragments".into());
        }
    }
    Ok(result)
}

/// Orthographic line removal across all bodies. Larger depth is closer to the eye.
/// A section keeps the half-space normal·p <= offset and is viewed along that plane normal.
pub fn project(meshes: &[Mesh], frame: Frame, cut: Option<Plane>) -> Result<Projection, String> {
    if meshes.is_empty() || meshes.len() > 16 {
        return Err("project 1..16 closed mesh bodies".into());
    }
    if !frame.right.is_finite() || !frame.up.is_finite() || !frame.toward_eye.is_finite() {
        return Err("view frame must have finite components".into());
    }
    let checked = Frame::new(frame.toward_eye, frame.up)?;
    if (checked.right - frame.right).len() > EPS || (checked.up - frame.up).len() > EPS || (checked.toward_eye - frame.toward_eye).len() > EPS {
        return Err("view frame must be orthonormal".into());
    }
    if let Some(p) = cut {
        let checked = Plane::new(p.normal, p.offset)?;
        if (checked.normal - p.normal).len() > EPS || (checked.offset - p.offset).abs() > EPS {
            return Err("plane must have a unit normal".into());
        }
    }
    if let Some(p) = cut
        && p.normal.dot(frame.toward_eye) < 1.0 - EPS
    {
        return Err("section view direction must match cutting-plane normal".into());
    }
    let mut triangles = Vec::<[Point; 3]>::new();
    let mut edges = Vec::<(usize, [usize; 2], Vec3, Vec3, bool)>::new();
    let mut sections = Vec::new();
    let mut work = 0usize;
    for (body, mesh) in meshes.iter().enumerate() {
        mesh.validate()?;
        let mut adjacency = BTreeMap::<(usize, usize), Vec<Vec3>>::new();
        for face in &mesh.faces {
            let points: Vec<_> = face.iter().map(|i| mesh.vertices.get(*i).copied().ok_or("face index out of range")).collect::<Result<_, _>>()?;
            let first = *points.first().ok_or("empty face")?;
            let second = *points.get(1).ok_or("face too short")?;
            let third = *points.get(2).ok_or("face too short")?;
            let normal = (second - first).cross(third - first).normalized();
            for (&a, &b) in face.iter().zip(face.iter().cycle().skip(1)).take(face.len()) {
                adjacency.entry((a.min(b), a.max(b))).or_default().push(normal);
            }
            let points = if let Some(p) = cut { clip_polygon(&points, p) } else { points };
            if let Some(&first) = points.first() {
                for pair in points.get(1..).unwrap_or_default().windows(2) {
                    if let [b, c] = pair {
                        if triangles.len() >= 4096 {
                            return Err("projection exceeds 4096 triangles".into());
                        }
                        triangles.push([frame.project(first), frame.project(*b), frame.project(*c)]);
                    }
                }
            }
        }
        for ((ai, bi), normals) in adjacency {
            if normals.len() == 2 && (normals[0] - normals[1]).len() < EPS {
                continue;
            }
            let mut a = *mesh.vertices.get(ai).ok_or("edge index out of range")?;
            let mut b = *mesh.vertices.get(bi).ok_or("edge index out of range")?;
            if let Some(p) = cut {
                let da = p.distance(a);
                let db = p.distance(b);
                if da > EPS && db > EPS {
                    continue;
                }
                if da > EPS {
                    a = a + (b - a) * (da / (da - db));
                } else if db > EPS {
                    b = a + (b - a) * (da / (da - db));
                }
            }
            if edges.len() >= 4096 {
                return Err("projection exceeds 4096 edges".into());
            }
            edges.push((body, [ai, bi], a, b, false));
        }
        let mut loops = Vec::new();
        if let Some(p) = cut {
            for path in cut_loops(mesh, p)? {
                let projected = path.iter().map(|p| frame.point(*p)).collect::<Vec<_>>();
                for (&a, &b) in path.iter().zip(path.iter().cycle().skip(1)).take(path.len()) {
                    if edges.len() >= 4096 {
                        return Err("projection exceeds 4096 edges".into());
                    }
                    edges.push((body, [usize::MAX, usize::MAX], a, b, true));
                }
                loops.push(projected);
            }
        }
        sections.push(loops);
    }
    if triangles.len() > 4096 || edges.len() > 4096 {
        return Err("projection geometry exceeds 4096 faces/edges".into());
    }
    let mut strokes = Vec::new();
    for (body, edge, a, b, section) in edges {
        let a = frame.project(a);
        let b = frame.project(b);
        let delta = b.p - a.p;
        let length = delta.len();
        if length < EPS {
            continue;
        }
        // Interval parameters are dimensionless; EPS is a model-space length.
        let parameter_eps = EPS / length;
        let mut hidden = Vec::new();
        for &triangle in &triangles {
            work += 1;
            if work > MAX_WORK {
                return Err("hidden-line pair-work limit exceeded".into());
            }
            if let Some(interval) = triangle_interval(a, b, triangle, parameter_eps) {
                hidden.push(interval);
            }
        }
        if let Some(p) = cut {
            for loops in &sections {
                cap_intervals(a, b, loops, p.offset, parameter_eps, &mut hidden);
            }
        }
        merge_intervals(&mut hidden, parameter_eps);
        let mut last = 0.0;
        for (lo, hi) in hidden {
            if lo - last > parameter_eps {
                strokes.push(Stroke { body, edge, a: a.p + delta * last, b: a.p + delta * lo, hidden: false, section });
            }
            strokes.push(Stroke { body, edge, a: a.p + delta * lo, b: a.p + delta * hi, hidden: true, section });
            last = hi;
        }
        if 1.0 - last > parameter_eps {
            strokes.push(Stroke { body, edge, a: a.p + delta * last, b: b.p, hidden: false, section });
        }
        if strokes.len() > 16384 {
            return Err("projection exceeds 16384 line fragments".into());
        }
    }
    let strokes = visible_precedence(strokes, &mut work)?;
    Ok(Projection { strokes, sections })
}

#[cfg(test)]
mod tests {
    use super::*;
    // Exercise section contours consumed by hatching and the final stroke split.
    #[test]
    fn diagonal_cube_section_joins_existing_vertices_and_edges_once() {
        let cube = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let plane = Plane::new(Vec3::new(1.0, 1.0, 0.0), 10.0).unwrap();
        let frame = Frame::new(plane.normal, Vec3::Z).unwrap();
        let projection = project(&[cube], frame, Some(plane)).unwrap();
        assert_eq!(projection.sections.len(), 1);
        assert_eq!(projection.sections[0].len(), 1);
        let path = &projection.sections[0][0];
        assert_eq!(path.len(), 4);
        for vertex in [Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(10.0, 0.0, 10.0), Vec3::new(0.0, 10.0, 10.0)] {
            assert_eq!(path.iter().filter(|p| p.dist(frame.point(vertex)) < EPS).count(), 1);
        }
        assert!((crate::shoelace(path).abs() - 100.0 * 2.0_f64.sqrt()).abs() < 1e-6);
        let outlines: Vec<_> = projection.strokes.iter().filter(|s| s.section && !s.hidden).collect();
        assert_eq!(outlines.len(), 4);
        let perimeter = outlines.iter().map(|s| s.a.dist(s.b)).sum::<f64>();
        assert!((perimeter - 20.0 * (1.0 + 2.0_f64.sqrt())).abs() < 1e-6);
    }
    #[test]
    fn large_coordinate_sections_close_without_merging_thin_features() {
        let frame = Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap();
        let plane = Plane::new(Vec3::Z, 333333333.3333333).unwrap();
        // The first shell used to miss shared-edge joins by about 6e-8; the
        // second ensures joining is not repaired by a model-size tolerance.
        for (min, max) in [(Vec3::ZERO, Vec3::new(1e9, 1e9, 1e9)), (Vec3::ZERO, Vec3::new(1e9, 1e-4, 1e9))] {
            let mesh = Mesh::cuboid(min, max).unwrap();
            let projection = project(&[mesh], frame, Some(plane)).unwrap();
            assert_eq!(projection.sections[0].len(), 1);
            let path = &projection.sections[0][0];
            assert_eq!(path.len(), 4);
            for expected in [Vec2::new(min.x, min.y), Vec2::new(max.x, min.y), Vec2::new(max.x, max.y), Vec2::new(min.x, max.y)] {
                assert_eq!(path.iter().filter(|p| p.dist(expected) < EPS).count(), 1);
            }
            let expected_area = (max.x - min.x) * (max.y - min.y);
            assert!((crate::shoelace(path).abs() / expected_area - 1.0).abs() < 1e-12);
        }
    }
    #[test]
    fn millimetre_occlusion_splits_billion_unit_edges() {
        let frame = Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap();
        for x in [500.0, 1e9 - 501.0] {
            let back = Mesh::cuboid(Vec3::ZERO, Vec3::new(1e9, 10.0, 10.0)).unwrap();
            let front = Mesh::cuboid(Vec3::new(x, -5.0, 20.0), Vec3::new(x + 1.0, 15.0, 30.0)).unwrap();
            let projection = project(&[back, front], frame, None).unwrap();
            for y in [0.0, 10.0] {
                let mut intervals: Vec<_> = projection
                    .strokes
                    .iter()
                    .filter(|s| s.body == 0 && s.a.y == y && s.b.y == y)
                    .map(|s| (s.a.x.min(s.b.x), s.a.x.max(s.b.x), s.hidden))
                    .collect();
                intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
                assert_eq!(intervals.len(), 3, "{intervals:?}");
                for ((lo, hi, hidden), (expected_lo, expected_hi, expected_hidden)) in
                    intervals.into_iter().zip([(0.0, x, false), (x, x + 1.0, true), (x + 1.0, 1e9, false)])
                {
                    assert!((lo - expected_lo).abs() < 1e-6);
                    assert!((hi - expected_hi).abs() < 1e-6);
                    assert_eq!(hidden, expected_hidden);
                }
            }
        }
    }
    #[test]
    fn nonfinite_public_frame_components_are_rejected_before_projection() {
        let cube = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let frame = Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap();
        for component in 0..9 {
            for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut invalid_frame = frame;
                let vector = match component / 3 {
                    0 => &mut invalid_frame.right,
                    1 => &mut invalid_frame.up,
                    _ => &mut invalid_frame.toward_eye,
                };
                match component % 3 {
                    0 => vector.x = invalid,
                    1 => vector.y = invalid,
                    _ => vector.z = invalid,
                }
                let error = project(std::slice::from_ref(&cube), invalid_frame, None).unwrap_err();
                assert_eq!(error, "view frame must have finite components");
            }
        }
        let projection = project(&[cube], frame, None).unwrap();
        assert_eq!(projection.strokes.len(), 4);
        assert!(projection.strokes.iter().all(|s| s.a.is_finite() && s.b.is_finite() && !s.hidden));
        assert!((projection.strokes.iter().map(|s| s.a.dist(s.b)).sum::<f64>() - 40.0).abs() < EPS);
    }
    #[test]
    fn cube_projection_has_visible_and_hidden_edges_and_real_sections() {
        let m = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let f = Frame::new(Vec3::new(1.0, 1.0, 1.0), Vec3::Z).unwrap();
        let p = project(std::slice::from_ref(&m), f, None).unwrap();
        assert!(p.strokes.iter().any(|s| s.hidden));
        assert!(p.strokes.iter().any(|s| !s.hidden));
        let f = Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap();
        let p = project(&[m], f, Some(Plane::new(Vec3::Z, 15.0).unwrap())).unwrap();
        assert_eq!(p.sections[0].len(), 1);
        let area = crate::shoelace(&p.sections[0][0]).abs();
        assert!((area - 200.0).abs() < 1e-6);
        assert!(p.strokes.iter().any(|s| s.section && !s.hidden));
    }
    #[test]
    fn another_body_occludes_only_covered_edge_intervals() {
        let back = Mesh::cuboid(Vec3::ZERO, Vec3::new(20.0, 10.0, 10.0)).unwrap();
        let front = Mesh::cuboid(Vec3::new(5.0, -5.0, 20.0), Vec3::new(15.0, 15.0, 30.0)).unwrap();
        let p = project(&[back, front], Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap(), None).unwrap();
        assert!(p.strokes.iter().any(|s| s.body == 0 && s.hidden && (s.a.x - 5.0).abs() < 1e-6 && (s.b.x - 15.0).abs() < 1e-6));
    }
    #[test]
    fn coincident_hidden_edges_do_not_overprint_visible_cube_outline() {
        let cube = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let projection = project(&[cube], Frame::new(Vec3::Z, Vec3::new(0.0, 1.0, 0.0)).unwrap(), None).unwrap();
        assert!(!projection.strokes.iter().any(|s| s.hidden));
        assert_eq!(projection.strokes.len(), 4);
        assert!(projection.strokes.iter().all(|s| s.a.x == s.b.x || s.a.y == s.b.y));
        assert!((projection.strokes.iter().map(|s| s.a.dist(s.b)).sum::<f64>() - 60.0).abs() < 1e-8);
    }
    #[test]
    fn open_or_inconsistently_oriented_shell_is_refused() {
        let mut m = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        m.faces.pop();
        assert!(m.validate().is_err());
    }
    #[test]
    fn intersecting_closed_shells_are_not_accepted_as_one_solid() {
        let a = Mesh::cuboid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let b = Mesh::cuboid(Vec3::new(5.0, 5.0, 5.0), Vec3::new(15.0, 15.0, 15.0)).unwrap();
        let mut vertices = a.vertices.into_owned();
        let offset = vertices.len();
        vertices.extend_from_slice(&b.vertices);
        let mut faces = a.faces;
        faces.extend(b.faces.into_iter().map(|f| f.into_iter().map(|i| i + offset).collect()));
        assert!(Mesh { vertices: vertices.into(), faces }.validate().is_err());
    }
}
