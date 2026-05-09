use serde::{Deserialize, Serialize};

/// A single public item extracted from the codebase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiItem {
    pub kind: ItemKind,
    pub name: String,
    /// Raw doc-comment text (concatenated `///` lines).
    pub doc: String,
    /// Human-readable signature string.
    pub signature: String,
    /// Module path relative to the parsed root, e.g. `["game_object", "sprite"]`.
    pub module_path: Vec<String>,
    /// Public methods attached via `impl` blocks.
    pub methods: Vec<ApiMethod>,
    /// Variants (enums only).
    pub variants: Vec<ApiVariant>,
    /// Named fields (structs only).
    pub fields: Vec<ApiField>,
    /// Raw generics string, empty if none.
    pub generics: String,
    /// Trait names this type implements (from `impl Trait for Type` blocks).
    pub traits_impl: Vec<String>,
}

impl ApiItem {
    /// Returns the first line of the doc comment, suitable for inline hints.
    pub fn doc_summary(&self) -> &str {
        self.doc.lines().next().map(str::trim).unwrap_or("")
    }

    /// Module path joined with `::`.
    pub fn module_str(&self) -> String {
        self.module_path.join("::")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ItemKind {
    Struct,
    Enum,
    Trait,
    Function,
    TypeAlias,
    Const,
}

impl ItemKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Struct    => "struct",
            Self::Enum      => "enum",
            Self::Trait     => "trait",
            Self::Function  => "fn",
            Self::TypeAlias => "type",
            Self::Const     => "const",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiMethod {
    pub name: String,
    pub doc: String,
    pub signature: String,
}

impl ApiMethod {
    pub fn doc_summary(&self) -> &str {
        self.doc.lines().next().map(str::trim).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiVariant {
    pub name: String,
    pub doc: String,
    pub fields: Vec<ApiField>,
}

impl ApiVariant {
    pub fn doc_summary(&self) -> &str {
        self.doc.lines().next().map(str::trim).unwrap_or("")
    }

    /// Render variant fields as a compact inline string, e.g. `{ path: String, volume: f32 }`.
    pub fn fields_inline(&self) -> String {
        if self.fields.is_empty() {
            return String::new();
        }
        let inner: Vec<String> = self.fields.iter()
            .map(|f| {
                if f.name.starts_with('_') {
                    f.ty.clone()
                } else {
                    format!("{}: {}", f.name, f.ty)
                }
            })
            .collect();
        format!("{{ {} }}", inner.join(", "))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiField {
    pub name: String,
    pub ty: String,
    pub doc: String,
}

// ── Extended Metadata for Advanced Tools ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeExample {
    pub title: String,
    pub description: String,
    pub code: String,
    pub context: String, // "common", "physics", "input", "advanced"
    pub source: String,  // where this came from
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiPattern {
    pub name: String,
    pub description: String,
    pub wrong_code: String,
    pub correct_code: String,
    pub consequence: String,
    pub affected_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitInfo {
    pub name: String,
    pub types_implementing: Vec<String>,
    pub required_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderInfo {
    pub base_type: String,
    pub builder_name: String,
    pub method_sequence: Vec<BuilderMethod>,
    pub finish_returns: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderMethod {
    pub name: String,
    pub params: Vec<(String, String)>, // (name, type)
    pub returns: String,
    pub doc: String,
    pub order_dependency: Option<String>, // method that must come before
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeRequirement {
    pub field: String,
    pub prerequisites: Vec<String>,
    pub incompatibilities: Vec<String>,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceChar {
    pub operation: String,
    pub complexity: String,
    pub cost_description: String,
    pub optimization_tips: Vec<String>,
}
