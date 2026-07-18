/// MCP (Model Context Protocol) server over stdio.
///
/// Implements just enough of the JSON-RPC MCP spec to register tools that
/// Copilot (or any MCP-capable host) can call during chat.
///
/// Tools exposed:
///   get_item        — full details on a named item
///   list_items      — list all items, optionally filtered by kind
///   search_items    — substring search across names and doc comments
///   get_variants    — all variants for a named enum (the key vocabulary tool)
///
/// Configure in .vscode/mcp.json:
///   {
///     "servers": {
///       "quartz-ctx": {
///         "type": "stdio",
///         "command": "quartz-ctx",
///         "args": ["serve", "--source", "src"]
///       }
///     }
///   }
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::{json, Value};
use walkdir::WalkDir;

use crate::model::{ApiItem, ItemKind};
use crate::{anti_patterns, behavior, examples, helpers, parser, patterns, timing};

// ── Source auto-reload ────────────────────────────────────────────────────────

/// How often (at most) we stat-scan the source trees for changes.
const RELOAD_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Cheap change fingerprint: FNV over every .rs path + mtime in all sources.
fn source_fingerprint(sources: &[(PathBuf, String)]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    let mut mix = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(1099511628211);
        }
    };
    for (path, _) in sources {
        for entry in WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "rs"))
        {
            mix(entry.path().to_string_lossy().as_bytes());
            if let Ok(meta) = entry.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                        mix(&d.as_secs().to_le_bytes());
                    }
                }
            }
        }
    }
    h
}

/// Re-parse the sources if anything changed since the last check.
/// The API served is therefore always ground truth — no server restarts needed
/// after engine edits.
fn maybe_reload(
    items: &mut Vec<ApiItem>,
    sources: &[(PathBuf, String)],
    last_check: &mut Instant,
    fingerprint: &mut u64,
) {
    if sources.is_empty() || last_check.elapsed() < RELOAD_CHECK_INTERVAL {
        return;
    }
    *last_check = Instant::now();

    let fp = source_fingerprint(sources);
    if fp == *fingerprint {
        return;
    }
    *fingerprint = fp;

    match parser::load_sources(sources) {
        Ok(new_items) if !new_items.is_empty() => {
            eprintln!(
                "quartz-ctx: source change detected — reloaded {} API items (was {})",
                new_items.len(), items.len()
            );
            *items = new_items;
        }
        Ok(_) => eprintln!("quartz-ctx: reload produced 0 items — keeping previous data"),
        Err(e) => eprintln!("quartz-ctx: reload failed ({e}) — keeping previous data"),
    }
}

// ── Curated-knowledge gating ──────────────────────────────────────────────────

/// Tools backed by hand-curated QUARTZ engine knowledge (examples.rs,
/// anti_patterns.rs, patterns.rs, behavior.rs, timing.rs, helpers.rs).
/// On a non-Quartz project this content would be confidently WRONG, so these
/// tools are only registered when serving the Quartz engine. The generic core
/// (get_api_context, get_item, get_variants, search_items, list_items,
/// find_related_types) is parsed live from source and works on ANY Rust project.
const CURATED_QUARTZ_TOOLS: &[&str] = &[
    "get_code_examples",
    "check_anti_patterns",
    "get_trait_implementations",
    "get_builder_methods",
    "validate_physics_config",
    "get_return_type_usage",
    "check_lifetime_constraints",
    "suggest_action_for_intent",
    "get_tick_loop_order",
    "explain_behavior",
    "get_usage_patterns",
    "get_engine_constants",
];

/// Curated Quartz knowledge is served only for the Quartz engine.
fn is_quartz(engine_name: &str) -> bool {
    engine_name.eq_ignore_ascii_case("quartz")
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn serve(items: Vec<ApiItem>, engine_name: &str, sources: Vec<(PathBuf, String)>) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    let mut items = items;
    let mut last_check = Instant::now();
    let mut fingerprint = source_fingerprint(&sources);

    eprintln!("quartz-ctx MCP server ready ({} items loaded)", items.len());

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("warn: could not parse request: {e}");
                continue;
            }
        };

        // Notifications have no id and need no response.
        let is_notification = req.get("id").is_none();

        let method = req["method"].as_str().unwrap_or("");

        if is_notification {
            // e.g. "notifications/initialized" — just swallow it
            continue;
        }

        let id = req["id"].clone();
        let params = req.get("params").cloned().unwrap_or(Value::Null);

        // Keep served data in sync with the source tree (throttled stat scan).
        if method == "tools/call" {
            maybe_reload(&mut items, &sources, &mut last_check, &mut fingerprint);
        }

        let result = match method {
            "initialize"  => Ok(initialize_result(engine_name)),
            "tools/list"  => Ok(tools_list_result(is_quartz(engine_name))),
            "tools/call"  => tools_call(&params, &items, engine_name),
            other         => Err(format!("unknown method: {other}")),
        };

        let response = match result {
            Ok(r)    => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err(msg) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": msg }
            }),
        };

        writeln!(out, "{}", serde_json::to_string(&response)?)?;
        out.flush()?;
    }

    Ok(())
}

// ── MCP protocol handlers ─────────────────────────────────────────────────────

fn initialize_result(engine_name: &str) -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": {
            "name": "quartz-ctx",
            "version": env!("CARGO_PKG_VERSION"),
            "description": format!("{engine_name} API reference tool")
        }
    })
}

fn tools_list_result(curated: bool) -> Value {
    let full = json!({
        "tools": [
            // ── Core Lookup Tools (Original 4) ────────────────────────────────────
            {
                "name": "get_item",
                "description": "Get complete details on a specific API item by exact name. \
                                Returns kind, full signature, doc comment, fields with types, \
                                all methods, enum variants, and trait implementations. \
                                Use this when you need the full picture of a type.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Exact name of the type, enum, trait, or function (case-sensitive)."
                        }
                    },
                    "required": ["name"]
                }
            },
            {
                "name": "list_items",
                "description": "List all public API items, optionally filtered by kind. \
                                Results grouped by type (Enums, Structs, Traits, Functions). \
                                Use this to discover what APIs are available, or get a quick reference of a category.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "description": "Filter by kind: struct, enum, trait, fn, type, const. \
                                           Leave blank to list all items grouped by category.",
                            "enum": ["struct", "enum", "trait", "fn", "type", "const"]
                        }
                    }
                }
            },
            {
                "name": "search_items",
                "description": "Search for API items by keyword, ranked by relevance. \
                                Searches item names (prioritized) and doc comments. \
                                Surfaces matching enum variants inline. \
                                Use this to find things when you don't know the exact name.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Keyword or substring to search for (case-insensitive). \
                                           E.g., 'position', 'gravity', 'camera'."
                        }
                    },
                    "required": ["query"]
                }
            },
            {
                "name": "get_variants",
                "description": "Get every variant of a named enum with full details. \
                                Returns all variants with their field types and documentation. \
                                **Primary use case for Quartz workflows**: call this before writing an Action, \
                                Condition, or GameEvent to find the exact variant you need. \
                                E.g., get_variants({\"name\": \"Action\"}) to see all available actions.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "Exact name of the enum (case-sensitive)."
                        }
                    },
                    "required": ["name"]
                }
            },
            // ── Tier 1: CRITICAL (Hallucination Prevention) ─────────────────────
            {
                "name": "get_code_examples",
                "description": "Get real code examples showing how to use a specific API. \
                                Prevents hallucinations by showing actual usage patterns from Quartz examples. \
                                Use this to see real working code before writing your own.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "api_item": {
                            "type": "string",
                            "description": "API item to find examples for (e.g., 'GameObject', 'Action', 'Camera')."
                        },
                        "context": {
                            "type": "string",
                            "description": "Optional context filter: 'common', 'physics', 'input', 'advanced'"
                        }
                    },
                    "required": ["api_item"]
                }
            },
            {
                "name": "check_anti_patterns",
                "description": "Check for known anti-patterns and mistakes in Quartz code. \
                                Prevents common bugs like SetPosition zeroing momentum, double borrows, etc. \
                                Use before writing code that touches physics, builders, or camera.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "type": {
                            "type": "string",
                            "description": "Type or API to check for anti-patterns (e.g., 'Action::SetPosition', 'GameObject', 'Camera')."
                        }
                    },
                    "required": ["type"]
                }
            },
            {
                "name": "get_trait_implementations",
                "description": "Check what traits a type implements or doesn't implement. \
                                Critical for generic code and understanding type compatibility. \
                                E.g., can you use this in a where T: Clone? Does it implement Copy?",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "type_name": {
                            "type": "string",
                            "description": "Type name to check (e.g., 'GameObject', 'Action', 'GameEvent')."
                        }
                    },
                    "required": ["type_name"]
                }
            },
            // ── Tier 2: HIGH-VALUE (Reliability Improvements) ───────────────────
            {
                "name": "get_builder_methods",
                "description": "Get all builder methods for a type and their correct sequence. \
                                Ensures builder chains are correct and complete. \
                                Use this when building complex objects like GameObject or Scene.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "base_type": {
                            "type": "string",
                            "description": "Base type that has a builder (e.g., 'GameObject', 'Scene', 'Camera')."
                        }
                    },
                    "required": ["base_type"]
                }
            },
            {
                "name": "validate_physics_config",
                "description": "Validate a physics configuration to catch incompatible settings. \
                                Prevents invalid physics setups before compilation. \
                                Example: check if gravity mode and collision mode are compatible.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "gravity": {"type": "string", "description": "Gravity mode"},
                        "collision_mode": {"type": "string", "description": "Collision mode"},
                        "friction": {"type": "number"}
                    }
                }
            },
            {
                "name": "get_return_type_usage",
                "description": "Find out what you can do with the return value of a method. \
                                Shows methods available on the return type and common usage patterns. \
                                E.g., you called get_velocity(), what methods does Velocity have?",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "method": {
                            "type": "string",
                            "description": "Full method path (e.g., 'GameObject::get_velocity', 'Canvas::camera')."
                        }
                    },
                    "required": ["method"]
                }
            },
            {
                "name": "find_related_types",
                "description": "Discover related APIs and types for a concept. \
                                Helps find the right API when you don't know exact names. \
                                E.g., 'collision detection' → CollisionMode, GameEvent::Collision, etc.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Concept or keyword to find related APIs for."
                        },
                        "related_to": {
                            "type": "string",
                            "description": "Optional: relate it to a specific type or context."
                        }
                    },
                    "required": ["query"]
                }
            },
            // ── Tier 3: ADVANCED (Safety & Performance) ──────────────────────────
            {
                "name": "check_lifetime_constraints",
                "description": "Check if a method's return value can be held across different scopes. \
                                Prevents borrow checker errors and runtime panics. \
                                E.g., can I hold this Ref across a frame? When does it panic?",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "method": {
                            "type": "string",
                            "description": "Method to check (e.g., 'canvas.get_game_object', 'shared.get')."
                        },
                        "holds_across": {
                            "type": "string",
                            "description": "What scope you want to hold it across: 'frame', 'tick', 'scope', 'loop'."
                        }
                    },
                    "required": ["method"]
                }
            },
            {
                "name": "suggest_action_for_intent",
                "description": "Given an intent (what you want to do), suggest the right Action or method. \
                                Reduces hallucination by suggesting real APIs for common intents. \
                                E.g., intent='make object spin' → Action::SetRotation, Action::ApplyTorque, etc.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "intent": {
                            "type": "string",
                            "description": "What you want to do in plain English (e.g., 'jump', 'move smoothly', 'spin')."
                        },
                        "object_type": {
                            "type": "string",
                            "description": "Optional: the type you're working with (e.g., 'GameObject', 'Camera')."
                        }
                    },
                    "required": ["intent"]
                }
            },
            // ── Phase 1 Additions: Behavioral & Semantic Knowledge ──────────────────
            {
                "name": "get_tick_loop_order",
                "description": "Get the complete 13-step tick loop execution order. \
                                Shows what runs when each frame: on_update, held-keys, physics, camera, etc. \
                                Critical for understanding timing bugs and event firing order. \
                                Use this when code behavior doesn't match expectations (esp. timing, physics, camera).",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "explain_behavior",
                "description": "Explain Quartz behavioral rules not visible in type signatures. \
                                Covers: when events fire, modifier handling, physics order, input timing, \
                                rendering, hot-reload latency, text rendering, and more. \
                                Use this to understand 'why' engine behaves the way it does.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "category": {
                            "type": "string",
                            "description": "Filter by category: input, physics, rendering, file_watching, text",
                            "enum": ["input", "physics", "rendering", "file_watching", "text"]
                        },
                        "query": {
                            "type": "string",
                            "description": "Optional: search within category (e.g., 'modifiers', 'hot-reload', 'camera')"
                        }
                    }
                }
            },
            {
                "name": "get_usage_patterns",
                "description": "Get real, working code examples extracted from api.txt. \
                                Shows patterns for: multi-span colored text, word-wrapping, object pooling, \
                                input handling with modifiers, hot-reload config, collision layers, and more. \
                                Use this to see 'how' to correctly use complex APIs.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Pattern name or category to look up (e.g., 'text', 'pooling', 'input', 'hot-reload')"
                        }
                    }
                }
            },
            {
                "name": "get_engine_constants",
                "description": "Get calibrated engine constants for calculations. \
                                Returns: tick delta (0.016s), hot-reload poll (0.5s), font scale (160.0), \
                                line height recommendations (1.35x body, 1.55x monospace). \
                                Use this for frame-locked timing, performance tuning, and text layout.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "constant": {
                            "type": "string",
                            "description": "Specific constant name (e.g., 'TICK_DELTA', 'FONT_SCALE_FACTOR'), or leave blank for all"
                        }
                    }
                }
            },
            // ── Compact context injection ────────────────────────────────────────
            {
                "name": "get_api_context",
                "description": "Get a compact, budgeted API context packet for a task. Pass a task \
                                description or keywords (e.g. 'spawn pooled bullets with collision and sound') \
                                and receive the most relevant types, enum variant names, and method signatures \
                                in minimal form — one call instead of several get_item/search_items round trips. \
                                Use this FIRST when starting a coding task; drill into specifics with \
                                get_variants/get_item afterwards.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "hint": {
                            "type": "string",
                            "description": "Task description or keywords driving relevance ranking."
                        },
                        "max_chars": {
                            "type": "integer",
                            "description": "Output budget in characters (default 4000)."
                        },
                        "origin": {
                            "type": "string",
                            "description": "Optional origin filter: e.g. 'quartz', 'synful-quartz', 'path-forge'."
                        }
                    },
                    "required": ["hint"]
                }
            }
        ]
    });

    if curated {
        return full;
    }

    // Non-Quartz project: expose only the generic, source-parsed tools.
    let tools: Vec<Value> = full["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|t| {
                    let name = t["name"].as_str().unwrap_or("");
                    !CURATED_QUARTZ_TOOLS.contains(&name)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    json!({ "tools": tools })
}

// ── Tool dispatch ─────────────────────────────────────────────────────────────

fn tools_call(params: &Value, items: &[ApiItem], engine_name: &str) -> Result<Value, String> {
    let tool_name = params["name"]
        .as_str()
        .ok_or("missing tool name")?;

    if !is_quartz(engine_name) && CURATED_QUARTZ_TOOLS.contains(&tool_name) {
        return Err(format!(
            "tool `{tool_name}` serves hand-curated QUARTZ engine knowledge and is disabled \
             for engine '{engine_name}' — its answers would not apply to this codebase. \
             Use the generic source-parsed tools instead: get_api_context, get_item, \
             get_variants, search_items, list_items, find_related_types."
        ));
    }

    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let text = match tool_name {
        "get_item"                    => tool_get_item(&args, items),
        "list_items"                  => tool_list_items(&args, items),
        "search_items"                => tool_search_items(&args, items),
        "get_variants"                => tool_get_variants(&args, items),
        "get_api_context"             => tool_get_api_context(&args, items),
        "get_code_examples"           => tool_get_code_examples(&args, items),
        "check_anti_patterns"         => tool_check_anti_patterns(&args, items),
        "get_trait_implementations"   => tool_get_trait_implementations(&args, items),
        "get_builder_methods"         => tool_get_builder_methods(&args, items),
        "validate_physics_config"     => tool_validate_physics_config(&args),
        "get_return_type_usage"       => tool_get_return_type_usage(&args, items),
        "find_related_types"          => tool_find_related_types(&args, items),
        "check_lifetime_constraints"  => tool_check_lifetime_constraints(&args),
        "suggest_action_for_intent"   => tool_suggest_action_for_intent(&args),
        // ── Phase 1 additions ──
        "get_tick_loop_order"         => tool_get_tick_loop_order(&args),
        "explain_behavior"            => tool_explain_behavior(&args),
        "get_usage_patterns"          => tool_get_usage_patterns(&args),
        "get_engine_constants"        => tool_get_engine_constants(&args),
        other                         => Err(format!("unknown tool: {other}")),
    }?;

    Ok(json!({
        "content": [{ "type": "text", "text": text }]
    }))
}

// ── Tool implementations ──────────────────────────────────────────────────────

fn tool_get_item(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("missing `name`")?;

    // Items keep source order: the first match is from the primary engine.
    let matches: Vec<&ApiItem> = items.iter().filter(|i| i.name == name).collect();
    let item = *matches.first()
        .ok_or_else(|| format!("no item named `{name}` found"))?;

    let mut out = format!("# `{}` ({})\n\n", item.name, item.kind.label());

    if !item.origin.is_empty() {
        out.push_str(&format!("origin: `{}`", item.origin));
        let others: Vec<&str> = matches.iter().skip(1)
            .map(|i| i.origin.as_str())
            .filter(|o| !o.is_empty())
            .collect();
        if !others.is_empty() {
            out.push_str(&format!("  (also defined in: {})", others.join(", ")));
        }
        out.push_str("\n\n");
    }

    if !item.module_str().is_empty() {
        out.push_str(&format!("module: `{}`\n\n", item.module_str()));
    }
    if !item.doc.is_empty() {
        out.push_str(&format!("{}\n\n", item.doc));
    }

    out.push_str(&format!("```rust\n{}\n```\n\n", item.signature));

    if !item.fields.is_empty() {
        out.push_str("## Fields\n\n");
        for f in &item.fields {
            let doc = if f.doc.is_empty() { String::new() } else { format!(" — {}", f.doc) };
            out.push_str(&format!("- `{}: {}`{}\n", f.name, f.ty, doc));
        }
        out.push('\n');
    }

    if !item.variants.is_empty() {
        out.push_str("## Variants\n\n");
        for v in &item.variants {
            let fields = v.fields_inline();
            let shape = if fields.is_empty() {
                format!("`{}`", v.name)
            } else {
                format!("`{}` `{}`", v.name, fields)
            };
            let doc = if v.doc.is_empty() { String::new() } else { format!(" — {}", v.doc_summary()) };
            out.push_str(&format!("- {}{}\n", shape, doc));
        }
        out.push('\n');
    }

    if !item.methods.is_empty() {
        out.push_str("## Methods\n\n");
        for m in &item.methods {
            let doc = if m.doc.is_empty() { String::new() } else { format!("\n  {}", m.doc_summary()) };
            out.push_str(&format!("- `{}`{}\n", m.signature, doc));
        }
        out.push('\n');
    }

    if !item.traits_impl.is_empty() {
        out.push_str(&format!("**Implements:** {}\n", item.traits_impl.join(", ")));
    }

    Ok(out)
}

fn tool_list_items(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let kind_filter: Option<ItemKind> = match args["kind"].as_str() {
        Some("struct") => Some(ItemKind::Struct),
        Some("enum")   => Some(ItemKind::Enum),
        Some("trait")  => Some(ItemKind::Trait),
        Some("fn")     => Some(ItemKind::Function),
        Some("type")   => Some(ItemKind::TypeAlias),
        Some("const")  => Some(ItemKind::Const),
        Some(other)    => return Err(format!("unknown kind `{other}`")),
        None           => None,
    };

    let filtered: Vec<_> = items
        .iter()
        .filter(|i| kind_filter.as_ref().map_or(true, |k| &i.kind == k))
        .collect();

    if filtered.is_empty() {
        return Ok("No items found.".into());
    }

    let mut out = String::new();

    // Group by kind for readability when listing everything
    if kind_filter.is_none() {
        for (label, kind) in &[
            ("Enums",      ItemKind::Enum),
            ("Structs",    ItemKind::Struct),
            ("Traits",     ItemKind::Trait),
            ("Functions",  ItemKind::Function),
            ("Type Aliases / Constants", ItemKind::TypeAlias),
        ] {
            let group: Vec<_> = filtered.iter().filter(|i| &i.kind == kind).collect();
            if group.is_empty() { continue; }
            out.push_str(&format!("## {}\n", label));
            for item in group {
                let doc = if item.doc_summary().is_empty() { String::new() } else { format!(" — {}", item.doc_summary()) };
                out.push_str(&format!("- `{}`{}\n", item.name, doc));
            }
            out.push('\n');
        }
    } else {
        for item in filtered {
            let doc = if item.doc_summary().is_empty() { String::new() } else { format!(" — {}", item.doc_summary()) };
            out.push_str(&format!("- `{}` ({}){}\n", item.name, item.kind.label(), doc));
        }
    }

    Ok(out)
}

fn tool_search_items(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let query = args["query"]
        .as_str()
        .ok_or("missing `query`")?
        .to_lowercase();

    // Score each item for ranking: exact name matches first, then name contains, then doc matches
    let mut scored: Vec<(i32, &ApiItem)> = items
        .iter()
        .filter_map(|i| {
            let name_lower = i.name.to_lowercase();
            let doc_lower = i.doc.to_lowercase();
            let has_variant_match = i.variants.iter().any(|v| {
                v.name.to_lowercase().contains(&query)
                    || v.doc.to_lowercase().contains(&query)
            });

            let score = if name_lower == query {
                3000 // exact name match
            } else if name_lower.starts_with(&query) {
                2000 // name starts with query
            } else if name_lower.contains(&query) {
                1000 // name contains query
            } else if doc_lower.contains(&query) {
                100  // doc contains query
            } else if has_variant_match {
                50   // variant match
            } else {
                return None;
            };

            Some((score, i))
        })
        .collect();

    // Sort by score descending, then by name for stability
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    if scored.is_empty() {
        return Ok(format!("No items matching `{query}`."));
    }

    let mut out = format!("{} result(s) for `{query}` (sorted by relevance):\n\n", scored.len());
    for (_score, item) in scored {
        out.push_str(&format!("- `{}` ({}", item.name, item.kind.label()));
        if !item.module_str().is_empty() {
            out.push_str(&format!(", module: `{}`", item.module_str()));
        }
        if !item.origin.is_empty() {
            out.push_str(&format!(", origin: `{}`", item.origin));
        }
        out.push(')');
        if !item.doc_summary().is_empty() {
            out.push_str(&format!("\n  {}", item.doc_summary()));
        }
        out.push('\n');

        // Surface matching variants inline
        let matching_variants: Vec<_> = item
            .variants
            .iter()
            .filter(|v| {
                v.name.to_lowercase().contains(&query)
                    || v.doc.to_lowercase().contains(&query)
            })
            .collect();

        for v in matching_variants {
            let fields = v.fields_inline();
            let shape = if fields.is_empty() { v.name.clone() } else { format!("{} {}", v.name, fields) };
            out.push_str(&format!("  ├─ variant `{}`", shape));
            if !v.doc_summary().is_empty() {
                out.push_str(&format!(" — {}", v.doc_summary()));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    Ok(out)
}

fn tool_get_variants(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let name = args["name"].as_str().ok_or("missing `name`")?;

    let item = items
        .iter()
        .find(|i| i.name == name && i.kind == ItemKind::Enum)
        .ok_or_else(|| format!("no enum named `{name}` found"))?;

    if item.variants.is_empty() {
        return Ok(format!("`{name}` has no variants."));
    }

    let mut out = format!("# `{}` variants\n\n", item.name);
    if !item.doc.is_empty() {
        out.push_str(&format!("{}\n\n", item.doc));
    }

    for v in &item.variants {
        let fields = v.fields_inline();
        if fields.is_empty() {
            out.push_str(&format!("## `{}::{}`\n", item.name, v.name));
        } else {
            out.push_str(&format!("## `{}::{}` `{}`\n", item.name, v.name, fields));
        }

        if !v.doc.is_empty() {
            out.push_str(&format!("{}\n\n", v.doc));
        }

        if v.fields.len() > 1 {
            for f in &v.fields {
                let name = if f.name.starts_with('_') { "(positional)".into() } else { format!("`{}`", f.name) };
                let doc = if f.doc.is_empty() { String::new() } else { format!(" — {}", f.doc) };
                out.push_str(&format!("- {}: `{}`{}\n", name, f.ty, doc));
            }
            out.push('\n');
        }
    }

    Ok(out)
}

// ── New 12 Tools (Tier 1–3) ──────────────────────────────────────────────────

fn tool_get_code_examples(args: &Value, _items: &[ApiItem]) -> Result<String, String> {
    let api_item = args["api_item"].as_str().ok_or("missing `api_item`")?;

    let examples_vec = examples::get_code_examples_for_api(api_item);
    if examples_vec.is_empty() {
        return Ok(format!(
            "No examples found for `{api_item}`. Try `search_items` to find related APIs."
        ));
    }

    let mut out = format!("# Code Examples for `{api_item}`\n\n");
    for ex in examples_vec {
        out.push_str(&format!("## {}\n\n", ex.title));
        if !ex.description.is_empty() {
            out.push_str(&format!("{}\n\n", ex.description));
        }
        out.push_str("```rust\n");
        out.push_str(&ex.code);
        out.push_str("\n```\n\n");
        if !ex.context.is_empty() {
            out.push_str(&format!("*Context: {}*\n\n", ex.context));
        }
    }
    Ok(out)
}

fn tool_check_anti_patterns(args: &Value, _items: &[ApiItem]) -> Result<String, String> {
    let type_name = args["type"].as_str().ok_or("missing `type`")?;

    let patterns = anti_patterns::get_all_anti_patterns();
    let relevant: Vec<_> = patterns
        .iter()
        .filter(|p| p.affected_types.iter().any(|t| t.contains(type_name)))
        .collect();

    if relevant.is_empty() {
        return Ok(format!("No known anti-patterns for `{type_name}`."));
    }

    let mut out = format!("# Anti-Patterns for `{type_name}`\n\n");
    for pattern in relevant {
        out.push_str(&format!("## ⚠️ {}\n\n", pattern.name));
        out.push_str(&format!("{}\n\n", pattern.description));

        out.push_str("**❌ Wrong:**\n```rust\n");
        out.push_str(&pattern.wrong_code);
        out.push_str("\n```\n\n");

        out.push_str("**✅ Right:**\n```rust\n");
        out.push_str(&pattern.correct_code);
        out.push_str("\n```\n\n");

        out.push_str(&format!("**Consequence:** {}\n\n", pattern.consequence));
    }
    Ok(out)
}

fn tool_get_trait_implementations(args: &Value, _items: &[ApiItem]) -> Result<String, String> {
    let type_name = args["type_name"].as_str().ok_or("missing `type_name`")?;

    let matrix = helpers::get_trait_matrix(type_name);

    let mut out = format!("# Trait Implementations for `{}`\n\n", matrix.type_name);

    if !matrix.implements.is_empty() {
        out.push_str("## Implements\n\n");
        for t in &matrix.implements {
            out.push_str(&format!("- `{}`\n", t));
        }
        out.push('\n');
    }

    if !matrix.does_not_implement.is_empty() {
        out.push_str("## Does NOT implement\n\n");
        for t in &matrix.does_not_implement {
            out.push_str(&format!("- `{}`\n", t));
        }
        out.push('\n');
    }

    if matrix.implements.is_empty() && matrix.does_not_implement.is_empty() {
        out.push_str("No trait information available.\n");
    }

    Ok(out)
}

fn tool_get_builder_methods(args: &Value, _items: &[ApiItem]) -> Result<String, String> {
    let base_type = args["base_type"].as_str().ok_or("missing `base_type`")?;

    let examples_list = examples::get_builder_examples(base_type);
    if examples_list.is_empty() {
        return Ok(format!(
            "No builder examples found for `{base_type}`. Try `get_code_examples` instead."
        ));
    }

    let mut out = format!("# Builder Pattern for `{base_type}`\n\n");
    for ex in examples_list {
        out.push_str(&format!("## {}\n\n", ex.title));
        out.push_str("```rust\n");
        out.push_str(&ex.code);
        out.push_str("\n```\n\n");
    }
    Ok(out)
}

fn tool_validate_physics_config(_args: &Value) -> Result<String, String> {
    // Honest output: no automated validation is implemented yet. The previous
    // version claimed "All settings compatible" unconditionally, which trained
    // agents to trust a check that never ran.
    let mut out = String::from("# Physics Configuration Validation\n\n");
    out.push_str("**Status:** NO automated validation was performed — this tool is a heuristic checklist only.\n\n");
    out.push_str("**Manual checklist (verify each yourself against source):**\n");
    out.push_str("- Gravity direction matches your game's coordinate system (positive Y is down in Quartz).\n");
    out.push_str("- `collision_layer(0)` silently disables dynamic-to-dynamic collision — use a non-zero layer.\n");
    out.push_str("- Manually-controlled objects (obstacles, decorations) need `gravity = 0.0` or pooled instances accumulate momentum offscreen.\n");
    out.push_str("- `physics.resistance` is the damping field — `physics.friction` does not exist.\n");
    out.push_str("- TerrainCollisionPlugin `object_size` is a `(f32, f32)` tuple, not a scalar.\n\n");
    out.push_str("Use `get_item` on the physics types involved to verify exact field names and defaults.\n");
    Ok(out)
}

/// get_api_context — one-call compact context packet for a task hint.
///
/// Scores every item against the hint words and renders the winners in
/// minimal form (signatures, variant names, method names) within a character
/// budget. Replaces several get_item/search_items round trips at task start.
fn tool_get_api_context(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let hint = args["hint"].as_str().ok_or("missing `hint`")?;
    let budget = args.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(4000) as usize;
    let budget = budget.clamp(500, 20_000);
    let origin_filter = args.get("origin").and_then(|v| v.as_str()).unwrap_or("");

    // Tokenize the hint: lowercase words of length >= 3, deduped.
    let mut words: Vec<String> = hint
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() >= 3)
        .map(str::to_string)
        .collect();
    words.dedup();
    if words.is_empty() {
        return Err("hint contains no usable keywords (need words of 3+ characters)".to_string());
    }

    // Score every item as the sum of per-word signals.
    let mut scored: Vec<(i64, &ApiItem)> = items
        .iter()
        .filter(|i| origin_filter.is_empty() || i.origin == origin_filter)
        .filter_map(|item| {
            let name = item.name.to_lowercase();
            let module = item.module_str().to_lowercase();
            let doc = item.doc.to_lowercase();
            let mut score: i64 = 0;
            for w in &words {
                if &name == w                 { score += 300; }
                else if name.contains(w)      { score += 100; }
                if module.contains(w)         { score += 40; }
                if doc.contains(w)            { score += 30; }
                if item.variants.iter().any(|v| v.name.to_lowercase().contains(w)) { score += 25; }
                if item.methods.iter().any(|m| m.name.to_lowercase().contains(w))  { score += 25; }
            }
            if score > 0 { Some((score, item)) } else { None }
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    if scored.is_empty() {
        return Ok(format!("No API context found for `{hint}`. Try broader keywords or `list_items`."));
    }

    let mut out = format!("# API context for: {hint}\n\n");
    let mut included = 0usize;
    for (_score, item) in &scored {
        let mut block = String::new();
        let origin = if item.origin.is_empty() { String::new() } else { format!(", {}", item.origin) };
        block.push_str(&format!("### `{}` ({}{})", item.name, item.kind.label(), origin));
        if !item.doc_summary().is_empty() {
            block.push_str(&format!(" — {}", item.doc_summary()));
        }
        block.push('\n');

        match item.kind {
            ItemKind::Enum => {
                // Variant NAMES only — the vocabulary; get_variants gives fields.
                let names: Vec<&str> = item.variants.iter().map(|v| v.name.as_str()).collect();
                if !names.is_empty() {
                    block.push_str(&format!("variants ({}): {}\n", names.len(), names.join(", ")));
                }
            }
            _ => {
                // Up to 8 method signatures, truncated — the callable surface.
                for m in item.methods.iter().take(8) {
                    let mut sig = m.signature.clone();
                    if sig.len() > 100 { sig.truncate(97); sig.push_str("..."); }
                    block.push_str(&format!("  - `{sig}`\n"));
                }
                if item.methods.len() > 8 {
                    block.push_str(&format!("  - ... {} more (use get_item)\n", item.methods.len() - 8));
                }
                if !item.fields.is_empty() && item.methods.is_empty() {
                    let fields: Vec<String> = item.fields.iter().take(10)
                        .map(|f| format!("{}: {}", f.name, f.ty)).collect();
                    block.push_str(&format!("fields: {}\n", fields.join(", ")));
                }
            }
        }
        block.push('\n');

        if out.len() + block.len() > budget {
            let remaining = scored.len() - included;
            out.push_str(&format!("*(budget reached — {remaining} more relevant item(s); refine the hint or raise max_chars)*\n"));
            break;
        }
        out.push_str(&block);
        included += 1;
        if included >= 15 { break; }
    }

    out.push_str("\nDrill down with get_variants(<enum>) or get_item(<type>) for full field/doc detail.\n");
    Ok(out)
}

fn tool_get_return_type_usage(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let method = args["method"].as_str().ok_or("missing `method`")?;

    let borrow_info = helpers::get_borrow_info(method);
    if let Some(info) = borrow_info {
        let mut out = format!("# Return Type Usage for `{}`\n\n", info.method_name);
        out.push_str(&format!("**Returns:** `{}`\n\n", info.return_type));
        out.push_str(&format!("**Borrow Kind:** {}\n\n", info.borrow_kind));
        out.push_str(&format!("**Lifetime Notes:** {}\n\n", info.lifetime_notes));
        Ok(out)
    } else {
        // Try searching for the method in items
        let search_query = method.split("::").last().unwrap_or(method).to_lowercase();
        let results = helpers::find_related_apis(&search_query, items);
        if results.is_empty() {
            Err(format!("No information found for method `{method}`"))
        } else {
            let mut out = format!("# Related to `{}`\n\n", method);
            for item in results.iter().take(5) {
                out.push_str(&format!("- `{}` ({})\n", item.name, item.kind.label()));
            }
            Ok(out)
        }
    }
}

fn tool_find_related_types(args: &Value, items: &[ApiItem]) -> Result<String, String> {
    let query = args["query"].as_str().ok_or("missing `query`")?;

    let related = helpers::find_related_apis(query, items);
    if related.is_empty() {
        return Ok(format!("No related types found for `{query}`."));
    }

    let mut out = format!("# Related Types for `{query}`\n\n");
    for item in related.iter().take(10) {
        out.push_str(&format!("- `{}` ({})", item.name, item.kind.label()));
        if !item.doc_summary().is_empty() {
            out.push_str(&format!(" — {}", item.doc_summary()));
        }
        out.push('\n');
    }
    Ok(out)
}

fn tool_check_lifetime_constraints(args: &Value) -> Result<String, String> {
    let method = args["method"].as_str().ok_or("missing `method`")?;

    if let Some(borrow) = helpers::get_borrow_info(method) {
        let mut out = format!("# Lifetime Constraints for `{}`\n\n", borrow.method_name);
        out.push_str(&format!("**Return Type:** `{}`\n\n", borrow.return_type));
        out.push_str(&format!("**Borrow Kind:** {}\n\n", borrow.borrow_kind));
        out.push_str(&format!("**Constraints:** {}\n\n", borrow.lifetime_notes));
        out.push_str("**Safety Check:** Verify you drop/release the returned value before calling mutable methods on the same object.\n");
        Ok(out)
    } else {
        Ok(format!(
            "No specific lifetime constraints known for `{method}`. \
             Use `get_return_type_usage` or consult the Quartz documentation."
        ))
    }
}

fn tool_suggest_action_for_intent(args: &Value) -> Result<String, String> {
    let intent = args["intent"].as_str().ok_or("missing `intent`")?;
    let _object_type = args.get("object_type").and_then(|v| v.as_str()).unwrap_or("GameObject");

    let suggestions = helpers::suggest_action_for_intent(intent, _object_type);

    let mut out = format!("# Suggested Actions for: \"{intent}\"\n\n");
    out.push_str("**Matching approaches:**\n\n");
    for (i, sugg) in suggestions.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, sugg));
    }
    out.push('\n');
    out.push_str("Use `get_variants` on any suggested Action/Condition to see exact variants and fields.\n");
    Ok(out)
}

// ── Phase 1 Tool Implementations ───────────────────────────────────────────

fn tool_get_tick_loop_order(_args: &Value) -> Result<String, String> {
    let loop_order = timing::get_tick_loop_order();
    
    let mut out = String::from("# Quartz Tick Loop Execution Order (13 Steps)\n\n");
    out.push_str("Each frame runs in this exact order. Understanding the sequence is critical for timing bugs, physics, and event firing.\n\n");
    
    for step in loop_order {
        out.push_str(&format!("## Step {}: {}\n\n", step.step, step.name));
        out.push_str(&format!("**Description:** {}\n\n", step.description));
        
        if !step.preconditions.is_empty() {
            out.push_str("**Preconditions:**\n");
            for p in &step.preconditions {
                out.push_str(&format!("- {}\n", p));
            }
            out.push('\n');
        }
        
        out.push_str("**Effects:**\n");
        for e in &step.effects {
            out.push_str(&format!("- {}\n", e));
        }
        
        if let Some(note) = step.critical_note {
            out.push_str(&format!("\n⚠️ **CRITICAL:** {}\n", note));
        }
        out.push('\n');
    }
    
    Ok(out)
}

fn tool_explain_behavior(args: &Value) -> Result<String, String> {
    let category = args.get("category").and_then(|v| v.as_str());
    let query = args.get("query").and_then(|v| v.as_str());
    
    let rules = if let Some(cat) = category {
        behavior::get_behavior_rule(cat)
    } else {
        behavior::get_behavior_rules()
    };
    
    let filtered = if let Some(q) = query {
        rules.into_iter()
            .filter(|r| r.rule.to_lowercase().contains(&q.to_lowercase()))
            .collect::<Vec<_>>()
    } else {
        rules
    };
    
    if filtered.is_empty() {
        return Ok("No behavior rules found matching your query.".to_string());
    }
    
    let mut out = String::from("# Quartz Behavioral Rules\n\n");
    
    for rule in filtered {
        out.push_str(&format!("## {} — {}\n\n", rule.category.to_uppercase(), rule.rule));
        out.push_str(&format!("**When it applies:** {}\n\n", rule.when_applies));
        
        out.push_str("**Examples:**\n");
        for ex in &rule.examples {
            out.push_str(&format!("- {}\n", ex));
        }
        
        out.push_str(&format!("\n**Consequence:** {}\n\n", rule.consequence));
        out.push_str(&format!("*Source: {}*\n\n", rule.source_reference));
    }
    
    Ok(out)
}

fn tool_get_usage_patterns(args: &Value) -> Result<String, String> {
    let pattern_query = args.get("pattern").and_then(|v| v.as_str());
    
    let all_patterns = patterns::get_usage_patterns();
    
    let results = if let Some(query) = pattern_query {
        all_patterns.into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query.to_lowercase())
                    || p.category.to_lowercase().contains(&query.to_lowercase())
            })
            .collect::<Vec<_>>()
    } else {
        all_patterns
    };
    
    if results.is_empty() {
        return Ok("No usage patterns found. Try categories: text, pooling, input, file_watching, collision, rendering.".to_string());
    }
    
    let mut out = String::from("# Quartz Usage Patterns\n\n");
    out.push_str("Real-world examples extracted from api.txt documentation.\n\n");
    
    for pattern in results {
        out.push_str(&format!("## {} ({})\n\n", pattern.name, pattern.category));
        out.push_str(&format!("{}\n\n", pattern.description));
        
        out.push_str("```rust\n");
        out.push_str(pattern.code);
        out.push_str("\n```\n\n");
        
        out.push_str(&format!("**Context:** {}\n\n", pattern.context));
        out.push_str(&format!("*Source: {}*\n\n", pattern.source_reference));
    }
    
    Ok(out)
}

fn tool_get_engine_constants(args: &Value) -> Result<String, String> {
    let constant_name = args.get("constant").and_then(|v| v.as_str());
    
    let constants = if let Some(name) = constant_name {
        if let Some(c) = helpers::get_constant(name) {
            vec![c]
        } else {
            return Err(format!("Constant `{name}` not found"));
        }
    } else {
        helpers::get_engine_constants()
    };
    
    let mut out = String::from("# Quartz Engine Constants\n\n");
    out.push_str("Calibrated values used throughout the engine for timing, rendering, and physics.\n\n");
    
    for constant in constants {
        out.push_str(&format!("## `{}`\n\n", constant.name));
        out.push_str(&format!("**Value:** `{} {}`\n\n", constant.value, constant.unit));
        out.push_str(&format!("{}\n\n", constant.description));
        out.push_str(&format!("**Usage:** {}\n\n", constant.usage));
    }
    
    Ok(out)
}
