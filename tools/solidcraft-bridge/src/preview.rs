//! Display-only triangles from the upstream mesher, never a drawing or measurement source.
use super::{Budget, Error, Worker, ensure_response_size, units};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_kernel::MeshTolerance;

pub(super) const MAX_TRIANGLES: usize = 100_000;
pub(super) const MAX_VERTICES: usize = 200_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Query {
    source_sha256: String,
    #[serde(default = "default_detail")]
    detail: u8,
}

fn default_detail() -> u8 {
    4
}

impl Worker {
    pub(super) fn preview(&self, query: Query) -> Result<Value, Error> {
        let model = self.model.as_ref().ok_or(Error::NoModel)?;
        if query.source_sha256 != model.fingerprint.sha256 {
            return Err(Error::StaleModel);
        }
        if !(1..=8).contains(&query.detail) {
            return Err(Error::Parameters("preview detail must be in 1..8".into()));
        }
        let budget = Budget::new();
        let (mut triangles, mut vertices) = (0_usize, 0_usize);
        let mut bodies = Vec::with_capacity(model.bodies.len());
        let mut missing_total = 0_usize;
        for (placement, loaded) in model.bodies.iter().enumerate() {
            budget.check()?;
            let body = &loaded.body;
            // At least one triangle per face: admission precedes the mesher. Its own
            // per-face work limits and the supervisor deadline still apply; this is
            // not an OS memory sandbox or a certified tessellation-error bound.
            if body.face_count() > MAX_TRIANGLES - triangles {
                return Err(Error::Limit("preview triangle admission"));
            }
            let size = body.size();
            if !size.is_finite() || size <= 0.0 {
                return Err(Error::Geometry("preview body size is not finite and positive".into()));
            }
            let chord = (size * 0.01 / f64::from(query.detail)).max(1e-3);
            let tolerance = MeshTolerance::display(chord);
            let plan = body.mesh_plan(&tolerance).map_err(|e| Error::Geometry(format!("preview body {placement}: {e}")))?;
            budget.check()?;
            let (mesh, refused) = plan.evaluate();
            budget.check()?;
            if mesh.triangles.len() > MAX_TRIANGLES - triangles || mesh.positions.len() > MAX_VERTICES - vertices {
                return Err(Error::Limit("preview triangles or vertices"));
            }
            if mesh.positions.iter().any(|p| !p.is_finite())
                || mesh.triangles.iter().flatten().any(|&i| usize::try_from(i).ok().is_none_or(|i| i >= mesh.positions.len()))
                || mesh.tri_face.len() != mesh.triangles.len()
            {
                return Err(Error::Geometry("preview mesher returned invalid vertices, indices or face references".into()));
            }
            let mut faces = vec![false; body.face_count()];
            for &face in &mesh.tri_face {
                let face = usize::try_from(face)
                    .ok()
                    .and_then(|i| faces.get_mut(i))
                    .ok_or_else(|| Error::Geometry("preview face reference is out of range".into()))?;
                *face = true;
            }
            let missing_faces: Vec<_> = faces.iter().enumerate().filter_map(|(i, present)| (!present).then_some(i)).collect();
            missing_total += missing_faces.len();
            triangles += mesh.triangles.len();
            vertices += mesh.positions.len();
            let refused: Vec<_> = refused.into_iter().map(|face| json!({"face":face.face,"reason":face.reason})).collect();
            bodies.push(json!({"placement":placement,"requested_chord_mm":chord,"effective_chord_mm":chord.max(size*1e-6),
                "display_angle_radians":tolerance.angle,"positions":mesh.positions,"triangles":mesh.triangles,"tri_face":mesh.tri_face,
                "missing_faces":missing_faces,"refused":refused}));
        }
        let result = json!({"source":model.fingerprint,"units":units(),"purpose":"display_only_not_measurement_or_projection",
            "placement_convention":"world_coordinates_already_placed_do_not_transform_again","bodies":bodies,
            "quality":{"missing_faces":missing_total,"bounds_certified":false,
                "notes":"Approximate upstream display tessellation; requested chord is not a certified global error bound. Inspect missing/refused faces; never use preview triangles for drafting or dimensions."},
            "counts":{"triangles":triangles,"vertices":vertices},"detail":query.detail});
        ensure_response_size(&result)?;
        Ok(result)
    }
}
