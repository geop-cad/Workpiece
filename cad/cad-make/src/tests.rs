//! The command line's own tests.
use super::*;

#[test]
fn a_step_with_an_argument_its_kind_lacks_is_refused() {
    let recipe = |args: Value| json!({ "steps": [{ "op": "Hole", "id": "h", "args": args }] });
    // A known argument is let by (and so is an empty unknown one).
    assert!(made(&recipe(json!({ "diameter": { "value": 5.0 } })), "part.wpc").is_ok());
    let said = match made(&recipe(json!({ "next": true })), "part.wpc") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("refused"),
    };
    assert!(
        said.contains("step 0 (Hole h)") && said.contains("\"next\""),
        "{said}"
    );
    assert!(
        said.contains("extent"),
        "it names the valid arguments: {said}"
    );
}

#[test]
fn a_plane_is_said_as_every_step_says_a_reference() {
    // The origin's planes by their names; a datum and a face as any
    // step refers to them; the sketch-only forms of old are refused.
    assert!(plane(&json!("XY")).is_ok());
    assert_eq!(
        plane(&json!({ "Datum": "mid" })).unwrap(),
        Reference::datum("mid")
    );
    assert_eq!(
        plane(&json!({ "Topology": "base/end" })).unwrap(),
        Reference::topology("base/end")
    );
    assert!(plane(&json!({ "datum": "mid" })).is_err());
}

#[test]
fn a_sketch_of_an_assembly_projects_an_edge_of_a_placed_part() {
    let folder = std::env::temp_dir().join(format!("workpiece_cli_placed_{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let in_folder = |name: &str| folder.join(name).to_string_lossy().into_owned();
    let rect = |id: &str, w: f64, h: f64| {
        json!({ "id": id, "plane": "XY",
            "points": { "a": [0.0, 0.0], "b": [w, 0.0], "c": [w, h], "d": [0.0, h] },
            "lines": { "ab": ["a", "b"], "bc": ["b", "c"], "cd": ["c", "d"], "da": ["d", "a"] },
            "constraints": [["horizontal", "ab"], ["horizontal", "cd"],
                            ["vertical", "bc"], ["vertical", "da"]] })
    };
    let plate = json!({ "material": null, "steps": [
        { "sketch": rect("s", 40.0, 20.0) },
        { "op": "Extrude", "id": "plate",
          "args": { "sketch": { "Profile": "s" }, "distance": 10.0, "combine": "NewBody" } },
    ] });
    let (part, _) = made(&plate, &in_folder("plate.wpc")).unwrap();
    std::fs::write(in_folder("plate.wpc"), part.to_text().unwrap()).unwrap();
    // The assembly places it, projects the plate's top edge into a
    // sketch, lays a square on it and extrudes that.
    let mut square = rect("q", 10.0, 10.0);
    square["constraints"]
        .as_array_mut()
        .unwrap()
        .push(json!(["point_on", "a", "rim"]));
    square["project"] = json!({ "rim": { "Placed": { "instance": "p1",
        "source": { "Topology": "plate/end/e11" } } } });
    let assembly = json!({ "steps": [
        { "op": "AddPart", "id": "p1",
          "args": { "part": "plate.wpc", "hold": { "fixed": true, "mates": [] }, "bodies": [] },
          "pose": { "at": [0.0, 0.0, 5.0], "euler": [0.0, 0.0, 0.0] } },
        { "sketch": square },
        { "op": "Extrude", "id": "x",
          "args": { "sketch": { "Profile": "q" }, "distance": 3.0, "combine": "NewBody" } },
    ] });
    let made_in_folder = made(&assembly, &in_folder("asm.wpc"));
    let (assembly, _) = match made_in_folder {
        Ok(made) => made,
        Err(e) => panic!("{e:#}"),
    };
    let workspace = workspace_of(&in_folder("asm.wpc"), assembly).unwrap();
    let built = workpiece_part::Engine::inline()
        .build(
            &std::sync::Arc::new(workspace),
            &in_folder("asm.wpc"),
            None,
            &|_| true,
            &workpiece_part::Cancel::new(),
        )
        .unwrap();
    assert!(built.world.extent().is_some());
    std::fs::remove_dir_all(&folder).ok();
}

#[test]
fn make_and_report_say_the_same_of_one_part() {
    // The entry both hosts run: a recipe made into a part file, with its
    // report; then the same part file reported on.
    let folder = std::env::temp_dir().join(format!("workpiece_make_entry_{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let at = |name: &str| folder.join(name).to_string_lossy().into_owned();
    let recipe = json!({ "steps": [
        { "sketch": { "id": "s", "plane": "XY",
            "points": { "a": [0.0, 0.0], "b": [20.0, 0.0], "c": [20.0, 10.0], "d": [0.0, 10.0] },
            "lines": { "ab": ["a", "b"], "bc": ["b", "c"], "cd": ["c", "d"], "da": ["d", "a"] },
            "constraints": [["horizontal", "ab"], ["horizontal", "cd"], ["vertical", "bc"], ["vertical", "da"]] } },
        { "op": "Extrude", "id": "plate", "args": { "sketch": { "Profile": "s" }, "distance": 5.0, "combine": "NewBody" } },
    ] });
    files::write(&at("recipe.json"), recipe.to_string()).unwrap();
    let run = |args: &[&str]| {
        let lines = std::cell::RefCell::new(Vec::<String>::new());
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        command(&args, &|line| lines.borrow_mut().push(line.to_owned())).unwrap();
        lines.into_inner()
    };
    let made = run(&[
        "make",
        &at("recipe.json"),
        "-o",
        &at("plate.wpc"),
        "--report",
        &at("r.json"),
    ]);
    assert!(made.contains(&"ok      plate".to_owned()), "{made:?}");
    assert!(made.contains(&"0 of 2 steps failed".to_owned()), "{made:?}");
    let report: Value =
        serde_json::from_str(&files::read_to_string(&at("r.json")).unwrap()).unwrap();
    assert_eq!(report["failed_steps"], 0);
    let volume = report["bodies"][0]["volume"].as_f64().unwrap();
    assert!((volume - 1000.0).abs() < 1e-6, "{volume}");
    let reported = run(&["report", &at("plate.wpc")]);
    assert!(
        reported.contains(&"0 of 2 steps failed".to_owned()),
        "{reported:?}"
    );
    assert!(command(&["frobnicate".to_owned()], &|_| ()).is_err());
    std::fs::remove_dir_all(&folder).ok();
}
