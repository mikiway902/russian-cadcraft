use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

#[test]
fn actual_worker_obeys_jsonrpc_framing_and_notifications_have_no_effects() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-model-worker"))
        .arg("--allow-path")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    writeln!(input, "not JSON").unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","method":"v1.load","params":{"path":"/notification-must-not-load.step"}})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1,"method":"v1.capabilities"})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":2,"method":"v1.source_status"})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":3,"method":"v99.load","params":{}})).unwrap();
    writeln!(
        input,
        "{}",
        json!([{"jsonrpc":"2.0","id":4,"method":"v1.capabilities"},
        {"jsonrpc":"2.0","id":5,"method":"v1.capabilities"}])
    )
    .unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":"model-status","method":"v1.source_status","params":{}})).unwrap();
    writeln!(input, "{}", json!({"jsonrpc":"2.0","id":1.25,"method":"v1.load","params":{"path":"/fractional-must-not-load.sc"}})).unwrap();
    drop(input);
    let responses: Vec<Value> = BufReader::new(output).lines().map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect();
    assert_eq!(responses.len(), 7, "notifications must not emit a reply");
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[1]["id"], 1);
    assert_eq!(responses[1]["result"]["protocol"], "cadcraft-model-worker/1");
    assert_eq!(responses[2]["error"]["data"]["kind"], "no_model");
    assert_eq!(responses[3]["error"]["code"], -32601);
    assert_eq!(responses[4]["error"]["code"], -32600);
    assert_eq!(responses[5]["id"], "model-status");
    assert_eq!(responses[5]["error"]["data"]["kind"], "no_model");
    assert_eq!(responses[6]["error"]["code"], -32600);
    assert!(child.wait().unwrap().success());
}

#[test]
fn oversized_line_closes_the_real_worker_with_bounded_error_frame() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-model-worker"))
        .arg("--allow-path")
        .arg(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    input.write_all(&vec![b'x'; cadcraft_model_worker::MAX_MESSAGE_BYTES + 1]).unwrap();
    drop(input);
    let responses: Vec<Value> = BufReader::new(output).lines().map(|line| serde_json::from_str(&line.unwrap()).unwrap()).collect();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["error"]["code"], -32003);
    assert!(child.wait().unwrap().success());
}

#[test]
fn supervised_rpc_discovers_native_tokens_and_resolves_real_exact_geometry() {
    use solidcore_record::Vec3;
    use solidcraft_doc::{Dir, Document, FeatureKind, Operation, point_expr};
    fn rpc(input: &mut impl Write, output: &mut impl BufRead, id: u64, method: &str, params: Value) -> Value {
        writeln!(input, "{}", json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params})).unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], id);
        response
    }
    let root = std::env::temp_dir().canonicalize().unwrap();
    let path = root.join(format!("cadcraft-reference-rpc-{}.sc", std::process::id()));
    let mut doc = Document::new("RPC exact cylinder");
    doc.add_feature(
        FeatureKind::Cylinder {
            base: point_expr(Vec3::ZERO),
            axis: Dir(Vec3::Z),
            radius: "5".into(),
            height: "20".into(),
            operation: Operation::NewBody,
            references: vec![],
        },
        Some("Cylinder"),
    )
    .unwrap();
    std::fs::write(&path, doc.to_json()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_cadcraft-model-worker"))
        .arg("--allow-path")
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let capabilities = rpc(&mut input, &mut output, 1, "v1.capabilities", json!({}));
    assert_eq!(capabilities["result"]["references"]["protocol"], "cadcraft-references/1");
    assert_eq!(capabilities["result"]["section"]["protocol"], "cadcraft-section/2");
    let loaded = rpc(&mut input, &mut output, 2, "v1.load", json!({"path": path}));
    assert!(loaded.get("error").is_none(), "{loaded}");
    let sha = &loaded["result"]["source"]["sha256"];
    let discovered = rpc(&mut input, &mut output, 3, "v1.references", json!({"source_sha256": sha}));
    assert!(discovered.get("error").is_none(), "{discovered}");
    let placement = &discovered["result"]["placements"][0];
    assert_eq!(placement["native_identity"]["feature"], 1, "a project-local id, not a document UUID");
    let token = &placement["edges"][0]["reference"];
    assert!(token.as_str().unwrap().starts_with("cadcraft-reference/1:"));
    let resolved = rpc(&mut input, &mut output, 4, "v1.resolve_point", json!({"source_sha256": sha, "reference": token, "fraction": 0.25}));
    assert_eq!(resolved["result"]["status"], "resolved", "{resolved}");
    let p: Vec3 = serde_json::from_value(resolved["result"]["point"].clone()).unwrap();
    assert!((p.x * p.x + p.y * p.y - 25.).abs() < 1e-6 && p.z >= -1e-7 && p.z <= 20. + 1e-7, "{p:?}");
    let refused = rpc(&mut input, &mut output, 5, "v1.resolve_point", json!({"source_sha256": sha, "reference": token, "fraction": 1.01}));
    assert_eq!(refused["error"]["code"], -32602);
    let section = rpc(
        &mut input,
        &mut output,
        6,
        "v1.section",
        json!({"source_sha256":sha,
        "plane":{"origin":[0,0,10],"normal":[0,0,1]},"kept_side":"positive","direction":[0,0,1],"up":[0,1,0],"symmetry_axis":[0,1]}),
    );
    assert!(section.get("error").is_none(), "{section}");
    assert_eq!(section["result"]["protocol"], "cadcraft-section/2");
    let symmetry = &section["result"]["section"]["symmetry"];
    assert_eq!(symmetry.as_object().unwrap().len(), 4);
    assert_eq!(symmetry["axis"], json!([0., 1.]));
    assert_eq!(symmetry["symmetric"], true);
    assert!((1e-9..=1e-6).contains(&symmetry["tolerance_mm"].as_f64().unwrap()));
    assert_eq!(section["result"]["section"]["bodies"][0]["status"], "cut");
    let caps = section["result"]["section"]["caps"].as_array().unwrap();
    assert_eq!(caps.len(), 1);
    let cap: solidcore_record::Region2 = serde_json::from_value(caps[0]["region"].clone()).unwrap();
    use solidcraft_geom::Loop2Geom;
    assert!((cap.outer.signed_area() - 25. * std::f64::consts::PI).abs() < 1e-5);
    assert!(cap.holes.is_empty());
    let invalid = rpc(
        &mut input,
        &mut output,
        7,
        "v1.section",
        json!({"source_sha256":sha,
        "plane":{"origin":[0,0,10],"normal":[0,0,1]},"kept_side":"positive","direction":[0,0,-1],"up":[0,1,0]}),
    );
    assert_eq!(invalid["error"]["code"], -32602);
    let removed = rpc(
        &mut input,
        &mut output,
        8,
        "v1.section",
        json!({"source_sha256":sha,
        "plane":{"origin":[0,0,21],"normal":[0,0,1]},"kept_side":"positive","direction":[0,0,1],"up":[0,1,0]}),
    );
    assert_eq!(removed["result"]["section"]["status"], "empty");
    assert!(removed["result"]["section"].as_object().unwrap().contains_key("symmetry"));
    assert!(removed["result"]["section"]["symmetry"].is_null());
    assert!(removed["result"]["drawing"]["lines"].as_array().unwrap().is_empty());
    let after = rpc(&mut input, &mut output, 9, "v1.references", json!({"source_sha256":sha}));
    assert_eq!(after["result"], discovered["result"], "section topology must never replace native naming/evaluation state");
    let resolved_after = rpc(&mut input, &mut output, 10, "v1.resolve_point", json!({"source_sha256":sha,"reference":token,"fraction":0.25}));
    assert_eq!(resolved_after["result"], resolved["result"]);
    let invalid_axis = rpc(
        &mut input,
        &mut output,
        11,
        "v1.section",
        json!({"source_sha256":sha,
        "plane":{"origin":[0,0,10],"normal":[0,0,1]},"kept_side":"positive","direction":[0,0,1],"up":[0,1,0],"symmetry_axis":[1,1]}),
    );
    assert_eq!(invalid_axis["error"]["code"], -32602);
    drop(input);
    assert!(child.wait().unwrap().success());
    std::fs::remove_file(path).unwrap();
}
