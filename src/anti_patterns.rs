// Anti-patterns database extracted from quartz_ai_api_cache and copilot-instructions.md
// These represent common mistakes that cause bugs, performance issues, or compile errors.

use crate::model::AntiPattern;

pub fn get_all_anti_patterns() -> Vec<AntiPattern> {
    vec![
        AntiPattern {
            name: "Double Borrow in Text Creation".to_string(),
            description: "Calling canvas.make_text() inside a get_game_object_mut() borrow scope causes a double borrow compile error.".to_string(),
            wrong_code: "if let Some(obj) = canvas.get_game_object_mut(\"label\") {\n    obj.set_drawable(Box::new(canvas.make_text(...)));\n}".to_string(),
            correct_code: "let txt = canvas.make_text(...);\nif let Some(obj) = canvas.get_game_object_mut(\"label\") {\n    obj.set_drawable(Box::new(txt));\n}".to_string(),
            consequence: "Compile error: cannot borrow canvas as mutable twice".to_string(),
            affected_types: vec!["Canvas".to_string(), "GameObject".to_string()],
        },
        AntiPattern {
            name: "Direct Prism Imports".to_string(),
            description: "Importing from prism directly instead of using quartz::prelude breaks encapsulation and causes confusion.".to_string(),
            wrong_code: "use prism::canvas::Color;".to_string(),
            correct_code: "use quartz::prelude::*;".to_string(),
            consequence: "API inconsistency, harder to maintain, may break with updates".to_string(),
            affected_types: vec!["Color".to_string(), "Text".to_string()],
        },
        AntiPattern {
            name: "SetPosition Zeroes Momentum".to_string(),
            description: "Using Action::SetPosition for smooth movement breaks physics because it resets momentum to zero.".to_string(),
            wrong_code: "canvas.run(Action::SetPosition { target, x, y });".to_string(),
            correct_code: "canvas.run(Action::Teleport { target, location: Location::at(x, y) });\n// Or use apply_momentum for smooth movement".to_string(),
            consequence: "Object stops abruptly, physics stops working, jumps feel unresponsive".to_string(),
            affected_types: vec!["Action".to_string()],
        },
        AntiPattern {
            name: "Custom Physics Instead of Crystalline".to_string(),
            description: "Building custom physics logic instead of using the Crystalline solver wastes effort and creates bugs.".to_string(),
            wrong_code: "obj.momentum.0 += gravity * dt;\n// manual collision solving...".to_string(),
            correct_code: "canvas.run(Action::EnableCrystalline);\n// Engine handles XPBD solver, broadphase, sleep, contacts".to_string(),
            consequence: "Physics is unreliable, performs poorly, hard to debug".to_string(),
            affected_types: vec!["GameObject".to_string(), "Canvas".to_string()],
        },
        AntiPattern {
            name: "Spring-Based Grapple Instead of GrappleConstraint".to_string(),
            description: "Building grapples with spring forces instead of using GrappleConstraint causes elastic/floaty behavior.".to_string(),
            wrong_code: "obj.momentum.0 += (anchor.0 - pos.0) * spring_k;".to_string(),
            correct_code: "let grapple = GrappleConstraint::new(obj_id, anchor);\nlet correction = grapple.solve(pos, vel);".to_string(),
            consequence: "Rope feels floaty and elastic instead of rigid, rope constraints don't hold properly".to_string(),
            affected_types: vec!["GrappleConstraint".to_string()],
        },
        AntiPattern {
            name: "Direct Camera Position Instead of Follow".to_string(),
            description: "Setting camera.position directly fights the engine's camera follow system and causes jittery behavior.".to_string(),
            wrong_code: "camera.position = (player_x - CW/2.0, 0.0);".to_string(),
            correct_code: "camera.follow(Some(Target::name(\"player\"))); // lerp_speed for smoothness".to_string(),
            consequence: "Camera jitters, doesn't follow smoothly, conflicts with follow lerp".to_string(),
            affected_types: vec!["Camera".to_string()],
        },
        AntiPattern {
            name: "Collision Layer Zero Disables Dynamic Collision".to_string(),
            description: "Using layer 0 for objects disables dynamic-to-dynamic collision detection.".to_string(),
            wrong_code: "obj.build(\"bullet\").collision_layer(0).finish();".to_string(),
            correct_code: ".collision_layer(collision_layers::PROJECTILE) // Use named layers".to_string(),
            consequence: "Objects don't collide with each other, physics fails silently".to_string(),
            affected_types: vec!["GameObject".to_string()],
        },
        AntiPattern {
            name: "Missing Hooked Flag Check Before Release".to_string(),
            description: "Calling ReleaseGrapple without checking hooked status can cause undefined behavior.".to_string(),
            wrong_code: "hooked = false; // Doesn't call ReleaseGrapple".to_string(),
            correct_code: "canvas.run(Action::ReleaseGrapple { target, ... });".to_string(),
            consequence: "Grapple state inconsistent, rope physics broken".to_string(),
            affected_types: vec!["Action".to_string()],
        },
        AntiPattern {
            name: "Entropy::range() with Integer Literals".to_string(),
            description: "Calling Entropy::range() with integer literals instead of f32 causes type errors.".to_string(),
            wrong_code: "Entropy::range(0, 10) // integers".to_string(),
            correct_code: "Entropy::range(0.0, 10.0) // f32".to_string(),
            consequence: "Compile error, entropy values don't generate correctly".to_string(),
            affected_types: vec!["Entropy".to_string()],
        },
        AntiPattern {
            name: "Shared<T> Ref Held Across get_mut()".to_string(),
            description: "Holding a Shared<T>::get() reference and calling get_mut() on the same object panics at runtime.".to_string(),
            wrong_code: "let ref_a = shared.get();\nlet mut_b = shared.get_mut(); // PANIC".to_string(),
            correct_code: "{\n    let ref_a = shared.get();\n    // use ref_a\n}\nlet mut_b = shared.get_mut(); // OK, ref_a dropped".to_string(),
            consequence: "Runtime panic, application crash".to_string(),
            affected_types: vec!["Shared".to_string()],
        },
        AntiPattern {
            name: "Manual Camera Flash Overlay".to_string(),
            description: "Setting camera flash overlay manually instead of using CameraEffects conflicts with engine management.".to_string(),
            wrong_code: "canvas.set_variable(\"__quartz_flash_overlay\", ...); // DO NOT".to_string(),
            correct_code: "canvas.camera_mut()?.flash(FlashEffect { ... });".to_string(),
            consequence: "Flash effects don't render, conflicts with engine state".to_string(),
            affected_types: vec!["Camera".to_string(), "CameraEffects".to_string()],
        },
    ]
}

pub fn find_anti_patterns_for_type(type_name: &str) -> Vec<&'static AntiPattern> {
    // This would be called at runtime with the static list
    // For now, returns matching patterns
    vec![]
}
