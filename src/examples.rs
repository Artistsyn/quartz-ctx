// Code examples extracted from real Quartz usage in example.rs and test games
use crate::model::CodeExample;

pub fn get_code_examples_for_api(item_name: &str) -> Vec<CodeExample> {
    match item_name {
        "GameObject" | "GameObject::build" => vec![
            CodeExample {
                title: "Basic GameObject Creation".to_string(),
                description: "Create a simple game object with position, size, and image".to_string(),
                code: r#"
let player = GameObject::build("player")
    .position(400.0, 1600.0)
    .size(100.0, 100.0)
    .image(solid_circle(100.0, Color(80, 160, 255, 255)))
    .tag("player")
    .finish();
"#.to_string(),
                context: "common".to_string(),
                source: "quartz/example.rs".to_string(),
            },
            CodeExample {
                title: "GameObject with Physics".to_string(),
                description: "Create object with gravity and resistance for platformer physics".to_string(),
                code: r#"
let player = GameObject::build("player")
    .position(400.0, 1600.0)
    .size(100.0, 100.0)
    .gravity(1.2)
    .resistance(0.92, 1.0)
    .image(solid_circle(100.0, Color(80, 160, 255, 255)))
    .finish();
"#.to_string(),
                context: "physics".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        "Canvas" => vec![
            CodeExample {
                title: "Canvas Creation and Scene Loading".to_string(),
                description: "Initialize canvas and load a scene".to_string(),
                code: r#"
let mut canvas = Canvas::new(ctx, CanvasMode::Landscape);
build_scenes(&mut canvas);
canvas.load_scene("game");
canvas
"#.to_string(),
                context: "common".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        "Camera" => vec![
            CodeExample {
                title: "Camera Follow Setup".to_string(),
                description: "Configure camera to follow a player object".to_string(),
                code: r#"
let cam = Camera::new((CW * 3.0, CH), (CW, CH));
canvas.set_camera(cam);
if let Some(cam) = canvas.camera_mut() {
    cam.follow(Some(Target::name("player")));
}
"#.to_string(),
                context: "common".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        "Action" => vec![
            CodeExample {
                title: "Apply Momentum Action".to_string(),
                description: "Use Action to apply velocity to an object".to_string(),
                code: r#"
GameEvent::KeyHold {
    key: Key::Named(NamedKey::ArrowLeft),
    action: Action::apply_momentum(Target::name("player"), -10.0, 0.0),
    target: Target::name("player"),
    modifiers: None,
}
"#.to_string(),
                context: "input".to_string(),
                source: "quartz/example.rs".to_string(),
            },
            CodeExample {
                title: "Conditional Jump Action".to_string(),
                description: "Use when_if to apply action only when condition is met".to_string(),
                code: r#"
GameEvent::KeyPress {
    key: Key::Named(NamedKey::Space),
    action: Action::when_if(
        Condition::Grounded(Target::name("player")),
        Action::apply_momentum(Target::name("player"), 0.0, -32.0),
    ),
    target: Target::name("player"),
    modifiers: None,
}
"#.to_string(),
                context: "input".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        "Scene" => vec![
            CodeExample {
                title: "Scene Creation with Objects and Events".to_string(),
                description: "Build a complete scene with game objects and event handlers".to_string(),
                code: r#"
Scene::new("game")
    .with_object("player".into(), player)
    .with_object("ground".into(), ground)
    .with_event(
        GameEvent::KeyHold {
            key: Key::Named(NamedKey::ArrowLeft),
            action: Action::apply_momentum(Target::name("player"), -10.0, 0.0),
            target: Target::name("player"),
            modifiers: None,
        },
        Target::name("player"),
    )
    .on_enter(|canvas| {
        canvas.set_var("score", 0_u32);
    })
"#.to_string(),
                context: "common".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        _ => vec![],
    }
}

pub fn get_builder_examples(base_type: &str) -> Vec<CodeExample> {
    match base_type {
        "GameObject" => vec![
            CodeExample {
                title: "Builder Sequence Example".to_string(),
                description: "Proper order of builder methods for GameObject".to_string(),
                code: r#"
GameObject::build("player")
    .position(x, y)      // position first
    .size(w, h)          // then size
    .gravity(g)          // physics params
    .resistance(drag, ang_drag)
    .image(image)        // rendering
    .tag("player")       // metadata
    .finish()
"#.to_string(),
                context: "common".to_string(),
                source: "quartz/example.rs".to_string(),
            },
        ],
        _ => vec![],
    }
}

pub fn get_all_examples() -> Vec<CodeExample> {
    let mut examples = vec![];
    examples.extend(get_code_examples_for_api("GameObject"));
    examples.extend(get_code_examples_for_api("Canvas"));
    examples.extend(get_code_examples_for_api("Camera"));
    examples.extend(get_code_examples_for_api("Action"));
    examples.extend(get_code_examples_for_api("Scene"));
    examples
}
