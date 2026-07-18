use std::path::Path;

use anyhow::Result;
use quote::quote;
use syn::visit::Visit;
use walkdir::WalkDir;

use crate::model::*;

// ── Entry point ──────────────────────────────────────────────────────────────

/// Recursively parse all `.rs` files under `dir` and return every public API item found.
pub fn parse_dir(dir: &Path) -> Result<Vec<ApiItem>> {
    let mut all_items: Vec<ApiItem> = Vec::new();
    let mut orphan_impls: Vec<PendingImpl> = Vec::new();

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
    {
        let path = entry.path();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warn: could not read {}: {}", path.display(), e);
                continue;
            }
        };

        match syn::parse_file(&content) {
            Ok(file) => {
                let module_path = derive_module_path(dir, path);
                let (items, leftovers) = extract_items(&file, &module_path);
                all_items.extend(items);
                orphan_impls.extend(leftovers);
            }
            Err(e) => {
                eprintln!("warn: could not parse {}: {}", path.display(), e);
            }
        }
    }

    // Global second pass: attach impl blocks whose owning type lives in a
    // DIFFERENT file. Quartz spreads `impl Canvas` across 9 files
    // (canvas/actions.rs, conditions.rs, physics.rs, ...) — the old per-file
    // attachment silently discarded all of them, serving Canvas with zero
    // methods and gutting plugin surfaces.
    for pending in orphan_impls {
        if let Some(owner) = all_items.iter_mut().find(|i| i.name == pending.self_ty) {
            for method in pending.methods {
                // Dedupe by name: the same method can appear via re-parse or
                // cfg-gated duplicate definitions.
                if !owner.methods.iter().any(|m| m.name == method.name) {
                    owner.methods.push(method);
                }
            }
            if let Some(tr) = pending.trait_name {
                if !tr.is_empty() && !owner.traits_impl.contains(&tr) {
                    owner.traits_impl.push(tr);
                }
            }
        }
        // Types from outside the scanned roots (e.g. std types) stay unattached.
    }

    Ok(all_items)
}

/// Parse multiple source roots, tagging every item with its origin slug.
/// Order matters: the FIRST source is the primary engine — lookups that match
/// multiple origins prefer it.
pub fn load_sources(sources: &[(std::path::PathBuf, String)]) -> Result<Vec<ApiItem>> {
    let mut all = Vec::new();
    for (path, tag) in sources {
        let mut items = parse_dir(path)?;
        for item in &mut items {
            item.origin = tag.clone();
        }
        all.extend(items);
    }
    Ok(all)
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Convert a file path into a Rust module path relative to `base`.
/// `src/game_object/sprite.rs` → `["game_object", "sprite"]`
fn derive_module_path(base: &Path, file: &Path) -> Vec<String> {
    let relative = file.strip_prefix(base).unwrap_or(file);
    relative
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|s| s != "mod" && s != "lib" && s != "main")
        .collect()
}

fn is_public(vis: &syn::Visibility) -> bool {
    matches!(vis, syn::Visibility::Public(_))
}

fn extract_docs(attrs: &[syn::Attribute]) -> String {
    attrs
        .iter()
        .filter_map(|a| {
            if !a.path().is_ident("doc") {
                return None;
            }
            if let syn::Meta::NameValue(nv) = &a.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    return Some(s.value().trim().to_string());
                }
            }
            None
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn type_to_string(ty: &syn::Type) -> String {
    // quote! preserves spaces; clean them for readability
    quote!(#ty)
        .to_string()
        .replace(" :: ", "::")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
}

fn generics_to_string(generics: &syn::Generics) -> String {
    if generics.params.is_empty() {
        String::new()
    } else {
        quote!(#generics).to_string()
    }
}

fn sig_to_string(sig: &syn::Signature) -> String {
    quote!(#sig)
        .to_string()
        .replace(" :: ", "::")
        .replace("< ", "<")
        .replace(" >", ">")
}

// ── Visitor ──────────────────────────────────────────────────────────────────

fn extract_items(file: &syn::File, module_path: &[String]) -> (Vec<ApiItem>, Vec<PendingImpl>) {
    let mut visitor = ApiVisitor {
        items: Vec::new(),
        module_path: module_path.to_vec(),
        pending_impls: Vec::new(),
    };
    visitor.visit_file(file);
    let leftovers = visitor.flush_impls();
    (visitor.items, leftovers)
}

struct PendingImpl {
    self_ty: String,
    trait_name: Option<String>,
    methods: Vec<ApiMethod>,
}

struct ApiVisitor {
    items: Vec<ApiItem>,
    module_path: Vec<String>,
    /// `impl` blocks collected before the owning type may have been seen.
    pending_impls: Vec<PendingImpl>,
}

impl ApiVisitor {
    /// Attach collected impl blocks to items in THIS file (fast path).
    /// Impls whose owning type lives in another file are RETURNED so the
    /// caller can attach them in a global pass — never discarded.
    fn flush_impls(&mut self) -> Vec<PendingImpl> {
        let mut leftovers = Vec::new();
        for pending in self.pending_impls.drain(..) {
            if let Some(owner) = self.items.iter_mut().find(|i| i.name == pending.self_ty) {
                for method in pending.methods {
                    if !owner.methods.iter().any(|m| m.name == method.name) {
                        owner.methods.push(method);
                    }
                }
                if let Some(tr) = pending.trait_name {
                    if !tr.is_empty() && !owner.traits_impl.contains(&tr) {
                        owner.traits_impl.push(tr);
                    }
                }
            } else {
                leftovers.push(pending);
            }
        }
        leftovers
    }
}

impl<'ast> Visit<'ast> for ApiVisitor {
    // ── struct ────────────────────────────────────────────────────────────────
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if !is_public(&node.vis) {
            syn::visit::visit_item_struct(self, node);
            return;
        }

        let fields: Vec<ApiField> = match &node.fields {
            syn::Fields::Named(named) => named
                .named
                .iter()
                .filter(|f| is_public(&f.vis))
                .map(|f| ApiField {
                    name: f.ident.as_ref().map_or("_".into(), |i| i.to_string()),
                    ty: type_to_string(&f.ty),
                    doc: extract_docs(&f.attrs),
                })
                .collect(),
            _ => vec![],
        };

        let name = node.ident.to_string();
        let generics = generics_to_string(&node.generics);
        let sig = if fields.is_empty() {
            format!("pub struct {}{};", name, generics)
        } else {
            let field_strs: Vec<String> = fields.iter()
                .map(|f| format!("    pub {}: {},", f.name, f.ty))
                .collect();
            format!("pub struct {}{} {{\n{}\n}}", name, generics, field_strs.join("\n"))
        };

        self.items.push(ApiItem {
            kind: ItemKind::Struct,
            name,
            doc: extract_docs(&node.attrs),
            signature: sig,
            module_path: self.module_path.clone(),
            methods: vec![],
            variants: vec![],
            fields,
            generics,
            traits_impl: vec![],
            origin: String::new(),
        });

        syn::visit::visit_item_struct(self, node);
    }

    // ── enum ──────────────────────────────────────────────────────────────────
    fn visit_item_enum(&mut self, node: &'ast syn::ItemEnum) {
        if !is_public(&node.vis) {
            syn::visit::visit_item_enum(self, node);
            return;
        }

        let variants: Vec<ApiVariant> = node
            .variants
            .iter()
            .map(|v| {
                let fields = match &v.fields {
                    syn::Fields::Named(named) => named
                        .named
                        .iter()
                        .map(|f| ApiField {
                            name: f.ident.as_ref().map_or("_".into(), |i| i.to_string()),
                            ty: type_to_string(&f.ty),
                            doc: extract_docs(&f.attrs),
                        })
                        .collect(),
                    syn::Fields::Unnamed(unnamed) => unnamed
                        .unnamed
                        .iter()
                        .enumerate()
                        .map(|(i, f)| ApiField {
                            name: format!("_{}", i),
                            ty: type_to_string(&f.ty),
                            doc: String::new(),
                        })
                        .collect(),
                    syn::Fields::Unit => vec![],
                };
                ApiVariant {
                    name: v.ident.to_string(),
                    doc: extract_docs(&v.attrs),
                    fields,
                }
            })
            .collect();

        let name = node.ident.to_string();
        let generics = generics_to_string(&node.generics);

        self.items.push(ApiItem {
            kind: ItemKind::Enum,
            name,
            doc: extract_docs(&node.attrs),
            signature: format!("pub enum {}{}", node.ident, generics),
            module_path: self.module_path.clone(),
            methods: vec![],
            variants,
            fields: vec![],
            generics,
            traits_impl: vec![],
            origin: String::new(),
        });

        syn::visit::visit_item_enum(self, node);
    }

    // ── free function ─────────────────────────────────────────────────────────
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !is_public(&node.vis) {
            return;
        }

        self.items.push(ApiItem {
            kind: ItemKind::Function,
            name: node.sig.ident.to_string(),
            doc: extract_docs(&node.attrs),
            signature: sig_to_string(&node.sig),
            module_path: self.module_path.clone(),
            methods: vec![],
            variants: vec![],
            fields: vec![],
            generics: generics_to_string(&node.sig.generics),
            traits_impl: vec![],
            origin: String::new(),
        });
    }

    // ── trait ─────────────────────────────────────────────────────────────────
    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if !is_public(&node.vis) {
            syn::visit::visit_item_trait(self, node);
            return;
        }

        let methods: Vec<ApiMethod> = node
            .items
            .iter()
            .filter_map(|item| {
                if let syn::TraitItem::Fn(m) = item {
                    Some(ApiMethod {
                        name: m.sig.ident.to_string(),
                        doc: extract_docs(&m.attrs),
                        signature: sig_to_string(&m.sig),
                    })
                } else {
                    None
                }
            })
            .collect();

        let name = node.ident.to_string();
        let generics = generics_to_string(&node.generics);

        self.items.push(ApiItem {
            kind: ItemKind::Trait,
            name,
            doc: extract_docs(&node.attrs),
            signature: format!("pub trait {}{}", node.ident, generics),
            module_path: self.module_path.clone(),
            methods,
            variants: vec![],
            fields: vec![],
            generics,
            traits_impl: vec![],
            origin: String::new(),
        });

        syn::visit::visit_item_trait(self, node);
    }

    // ── impl block ────────────────────────────────────────────────────────────
    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        // Only care about impl blocks for named types (not `impl Trait for &dyn …`)
        let self_ty_name = match node.self_ty.as_ref() {
            syn::Type::Path(p) => p
                .path
                .segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default(),
            _ => return,
        };

        if self_ty_name.is_empty() {
            return;
        }

        let methods: Vec<ApiMethod> = node
            .items
            .iter()
            .filter_map(|item| {
                if let syn::ImplItem::Fn(m) = item {
                    if !is_public(&m.vis) {
                        return None;
                    }
                    Some(ApiMethod {
                        name: m.sig.ident.to_string(),
                        doc: extract_docs(&m.attrs),
                        signature: sig_to_string(&m.sig),
                    })
                } else {
                    None
                }
            })
            .collect();

        let trait_name = node.trait_.as_ref().map(|(_, path, _)| {
            path.segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_default()
        });

        self.pending_impls.push(PendingImpl {
            self_ty: self_ty_name,
            trait_name,
            methods,
        });

        // Don't recurse into impl — we've handled it manually.
    }

    // ── type alias ────────────────────────────────────────────────────────────
    fn visit_item_type(&mut self, node: &'ast syn::ItemType) {
        if !is_public(&node.vis) {
            return;
        }

        let ty = &node.ty;
        self.items.push(ApiItem {
            kind: ItemKind::TypeAlias,
            name: node.ident.to_string(),
            doc: extract_docs(&node.attrs),
            signature: format!(
                "pub type {} = {};",
                node.ident,
                type_to_string(ty)
            ),
            module_path: self.module_path.clone(),
            methods: vec![],
            variants: vec![],
            fields: vec![],
            generics: generics_to_string(&node.generics),
            traits_impl: vec![],
            origin: String::new(),
        });
    }

    // ── const ─────────────────────────────────────────────────────────────────
    fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
        if !is_public(&node.vis) {
            return;
        }

        let ty = &node.ty;
        // Capture the actual value expression (truncated) — agents asking for
        // engine constants need the real number, not an ellipsis.
        let expr = &node.expr;
        let mut value = quote!(#expr).to_string().replace(" :: ", "::");
        if value.len() > 60 {
            value.truncate(57);
            value.push_str("...");
        }
        self.items.push(ApiItem {
            kind: ItemKind::Const,
            name: node.ident.to_string(),
            doc: extract_docs(&node.attrs),
            signature: format!(
                "pub const {}: {} = {};",
                node.ident,
                type_to_string(ty),
                value
            ),
            module_path: self.module_path.clone(),
            methods: vec![],
            variants: vec![],
            fields: vec![],
            generics: String::new(),
            traits_impl: vec![],
            origin: String::new(),
        });
    }

    // ── inline module ─────────────────────────────────────────────────────────
    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if let Some((_, items)) = &node.content {
            let old_path = self.module_path.clone();
            self.module_path.push(node.ident.to_string());
            for item in items {
                self.visit_item(item);
            }
            self.module_path = old_path;
            // NOTE: no flush here — the single file-level flush in extract_items
            // attaches everything and returns cross-file leftovers to the caller.
            // An inner flush would drop leftover impls from this module.
        }
    }
}
