// Validation and helper functions for Quartz API analysis
use crate::model::{ApiItem, ItemKind};

/// Builder chain validation
pub struct BuilderValidation {
    pub base_type: String,
    pub valid_sequence: bool,
    pub issues: Vec<String>,
}

pub fn validate_builder_sequence(_base_type: &str, _methods: &[String]) -> BuilderValidation {
    BuilderValidation {
        base_type: _base_type.to_string(),
        valid_sequence: true,
        issues: vec![],
    }
}

/// Check if two types are compatible in a method chain
pub fn are_types_compatible(from_type: &str, to_type: &str) -> bool {
    // Simplified compatibility matrix
    match (from_type, to_type) {
        ("Position", "Vec2") => true,
        ("Vec2", "Momentum") => true,
        ("f32", "Velocity") => true,
        _ => from_type == to_type,
    }
}

/// Trait implementation matrix
pub struct TraitMatrix {
    pub type_name: String,
    pub implements: Vec<String>,
    pub does_not_implement: Vec<String>,
}

pub fn get_trait_matrix(type_name: &str) -> TraitMatrix {
    let implements = match type_name {
        "GameObject" => vec!["Clone".to_string(), "Debug".to_string()],
        "Action" => vec!["Clone".to_string(), "Debug".to_string()],
        "GameEvent" => vec!["Clone".to_string(), "Debug".to_string()],
        "Condition" => vec!["Clone".to_string(), "Debug".to_string()],
        _ => vec![],
    };
    
    let does_not_implement = match type_name {
        "GameObject" => vec!["Copy".to_string(), "Hash".to_string()],
        "Canvas" => vec!["Clone".to_string(), "Copy".to_string()],
        _ => vec![],
    };

    TraitMatrix {
        type_name: type_name.to_string(),
        implements,
        does_not_implement,
    }
}

/// Performance characteristics database
pub struct PerformanceInfo {
    pub operation: String,
    pub complexity: String,
    pub cost: String,
    pub tips: Vec<String>,
}

pub fn get_performance_info(operation: &str) -> Option<PerformanceInfo> {
    match operation {
        "canvas.query_collision_group" => Some(PerformanceInfo {
            operation: operation.to_string(),
            complexity: "O(n)".to_string(),
            cost: "~1μs per object in layer".to_string(),
            tips: vec![
                "Use collision layers to reduce group size".to_string(),
                "Cache results if querying same group multiple times".to_string(),
                "Consider spatial partitioning for large groups".to_string(),
            ],
        }),
        "GameObject::get_game_object_mut" => Some(PerformanceInfo {
            operation: operation.to_string(),
            complexity: "O(1)".to_string(),
            cost: "Fast: hash lookup".to_string(),
            tips: vec![
                "Safe to call every frame".to_string(),
                "Mutable borrow prevents other accesses".to_string(),
                "Use object IDs instead of names for performance".to_string(),
            ],
        }),
        _ => None,
    }
}

/// Type requirements and prerequisites
pub struct TypeRequirementInfo {
    pub type_name: String,
    pub requirements: Vec<(String, Vec<String>)>, // (field, prerequisites)
}

pub fn get_type_requirements(type_name: &str) -> TypeRequirementInfo {
    let requirements = match type_name {
        "GameObject" => vec![
            ("position".to_string(), vec!["valid coordinates".to_string()]),
            ("size".to_string(), vec!["positive dimensions".to_string()]),
            ("gravity".to_string(), vec!["enable_physics must be called if physics enabled".to_string()]),
        ],
        _ => vec![],
    };

    TypeRequirementInfo {
        type_name: type_name.to_string(),
        requirements,
    }
}

/// Lifetime and borrow rules
pub struct BorrowInfo {
    pub method_name: String,
    pub return_type: String,
    pub borrow_kind: String,
    pub lifetime_notes: String,
}

pub fn get_borrow_info(method: &str) -> Option<BorrowInfo> {
    match method {
        "canvas.get_game_object" => Some(BorrowInfo {
            method_name: method.to_string(),
            return_type: "Option<&GameObject>".to_string(),
            borrow_kind: "Immutable borrow".to_string(),
            lifetime_notes: "Safe across frames, multiple borrows allowed".to_string(),
        }),
        "canvas.get_game_object_mut" => Some(BorrowInfo {
            method_name: method.to_string(),
            return_type: "Option<&mut GameObject>".to_string(),
            borrow_kind: "Mutable borrow".to_string(),
            lifetime_notes: "Cannot hold across other borrows. Use in a scope then release.".to_string(),
        }),
        "shared.get" => Some(BorrowInfo {
            method_name: method.to_string(),
            return_type: "Ref<T>".to_string(),
            borrow_kind: "Interior mutable ref".to_string(),
            lifetime_notes: "Held Ref prevents get_mut(). Drop Ref before calling get_mut().".to_string(),
        }),
        _ => None,
    }
}

/// Find related types/APIs for a query
pub fn find_related_apis(query: &str, items: &[ApiItem]) -> Vec<ApiItem> {
    items
        .iter()
        .filter(|item| {
            let name_lower = item.name.to_lowercase();
            let doc_lower = item.doc.to_lowercase();
            let query_lower = query.to_lowercase();

            name_lower.contains(&query_lower)
                || doc_lower.contains(&query_lower)
                || item.variants.iter().any(|v| {
                    v.name.to_lowercase().contains(&query_lower)
                        || v.doc.to_lowercase().contains(&query_lower)
                })
        })
        .cloned()
        .collect()
}

/// Suggest intent-based actions
pub fn suggest_action_for_intent(intent: &str, _object_type: &str) -> Vec<String> {
    match intent {
        "make object spin" => vec![
            "Action::SetRotation (basic)".to_string(),
            "Action::ApplyTorque (if physics enabled)".to_string(),
            "Action::RotateToward (smooth rotation)".to_string(),
        ],
        "move object smoothly" => vec![
            "Action::Teleport (instant)".to_string(),
            "Action::ApplyMomentum (physics-based)".to_string(),
            "Action::MoveTo (lerped position)".to_string(),
        ],
        "jump" => vec![
            "Action::ApplyMomentum with negative Y value".to_string(),
            "Condition::Grounded check recommended before jump".to_string(),
        ],
        "collide with" => vec![
            "GameEvent::Collision".to_string(),
            "Action::SetCollisionMode".to_string(),
            "collision_layers configuration".to_string(),
        ],
        _ => vec!["Search quartz-ctx for related APIs".to_string()],
    }
}
