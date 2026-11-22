use super::*;
use cadcraft_doc::{
    model_projection::{Basis, Quality},
    model_section::{BodyDisposition, BodyStatus, KeptSide, Plane, SectionStatus},
};

#[test]
fn completed_section_status_uses_the_shown_language_before_render_scope() {
    for (language, expected) in [("ru", "Разрез удаляет все выбранные тела; вид не создан."), ("zh", "剖切移除了全部所选实体；未创建视图。")]
    {
        let mut app = CadApp::new(cadcraft_engine::extensions::new_session(), crate::Services::default());
        app.ui.interface_language = crate::i18n::Preference::parse(language).unwrap();
        // The UI intentionally shows English when no CJK font is available.
        let expected = match (language, app.language()) {
            ("zh", "en") => "Section removes every selected body; no view was created.",
            _ => expected,
        };
        let sha = "a".repeat(64);
        let metadata = json!({"protocol":"cadcraft-model-worker/1","path":"/models/locale.step",
            "source":{"sha256":sha,"bytes":512},"format":"step",
            "units":{"length":"mm","area":"mm2","volume":"mm3","angle":"radian","coordinate_system":"right_handed_z_up"},
            "metadata":{},"bodies":[{"name":"Body"}],"tree":{},"warnings":[],"report":{}});
        let id = models::apply_loaded(&mut app.session, metadata, None).unwrap()["id"].as_str().unwrap().to_owned();
        let section = Section {
            projection: Projection {
                entities: Vec::new(),
                source_sha256: sha.clone(),
                source_bytes: 512,
                basis: Basis { x: [-1., 0., 0.], y: [0., 1., 0.], direction: [0., 0., 1.] },
                quality: Quality {
                    unsupported_faces: 0,
                    uncertain_pieces: 0,
                    bounded_pieces: 0,
                    sampled_pieces: 0,
                    unsupported_trims: 0,
                    import_issues: 0,
                    open_bodies: 0,
                    analytic_complete: true,
                },
                lines: Vec::new(),
                unsupported: Vec::new(),
                prepare_reports: json!([]),
                drawing_stats: json!({}),
                source_mapping: "section_result_local_only".into(),
                visibility_contract: "raw_uncertain_and_covered_preserved_no_mesh_fallback".into(),
            },
            plane: Plane { origin: [0., 0., 101.], normal: [0., 0., 1.] },
            kept_side: KeptSide::Positive,
            status: SectionStatus::Empty,
            bodies: vec![BodyDisposition { source_body: 0, source_placement: 0, result_body: None, status: BodyStatus::Removed, topology: None }],
            caps: Vec::new(),
        };
        let state = app.session.state().unwrap();
        let (uid, revision) = (state.uid, state.revision);
        let (tx, rx) = channel();
        tx.send(Ok(Outcome::Section {
            section: Box::new(section),
            params: json!({"id":id,"plane":{"origin":[0,0,101],"normal":[0,0,1]},"kept_side":"positive"}),
        }))
        .unwrap();
        app.models.job = Some(Job {
            uid,
            document_revision: Some(revision),
            space: None,
            guard: Some(Arc::new(Guard { id, revision: 1, path: "/models/locale.step".into(), sha256: sha })),
            kind: Kind::Section,
            started: Instant::now(),
            rx,
        });
        let context = egui::Context::default();
        let mut output = context.run_ui(Default::default(), |ui| app.ui_frame_begin(ui));
        output.textures_delta.clear();
        assert!(app.models.error.is_none(), "{:?}", app.models.error);
        assert_eq!(app.status.as_ref().map(|(message, _)| message.as_str()), Some(expected));
        assert!(app.models.job.is_none());
    }
}
