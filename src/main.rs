#![allow(dead_code, unused_imports, unused_variables)]

mod anti_patterns;
mod behavior;
mod examples;
mod helpers;
mod mcp;
mod model;
mod parser;
mod patterns;
mod render;
mod timing;

use std::path::Path;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{error::ErrorKind, Parser, Subcommand};
use serde_json::json;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(name = "quartz-ctx", version, about = "API context tool and MCP skill server")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate static markdown documentation tree at docs/<scraped-directory>/.
    /// 
    /// Run this once after API changes to refresh the docs that Copilot reads.
    /// Output includes INDEX.md (entry point), vocabulary.md (enums), types.md, traits.md,
    /// functions.md, and api-graph.json.
    /// 
    /// Example:
    ///   quartz-ctx generate --source quartz/src --name Quartz
    /// 
    /// Then add to .github/copilot-instructions.md:
    ///   Before writing Quartz code, review docs/quartz/INDEX.md
    Generate(GenerateArgs),

    /// Run as an MCP stdio skill server for live API lookups.
    /// 
    /// Copilot calls this in real-time during chat to look up exact signatures,
    /// list available variants, search for APIs, etc. All data is loaded once at startup.
    /// 
    /// Configure in .vscode/mcp.json:
    ///   {
    ///     "servers": {
    ///       "quartz-ctx": {
    ///         "type": "stdio",
    ///         "command": "quartz-ctx",
    ///         "args": ["serve", "--source", "quartz/src", "--name", "Quartz"]
    ///       }
    ///     }
    ///   }
    /// 
    /// Then Copilot can call tools like:
    ///   - get_variants({\"name\": \"Action\"})
    ///   - search_items({\"query\": \"gravity\"})
    ///   - list_items({\"kind\": \"enum\"})
    Serve(ServeArgs),

    /// Run startup diagnostics and source validation.
    ///
    /// Helpful when MCP fails to boot or source paths are incorrect.
    ///
    /// Example:
    ///   quartz-ctx selfcheck --source quartz/src --name Quartz
    Selfcheck(SelfcheckArgs),
}

// ── generate ──────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
struct GenerateArgs {
    /// Source directory to analyse (scanned recursively for .rs files).
    #[arg(short, long, default_value = "src")]
    source: PathBuf,

    /// Output root. Context tree lands at <output>/docs/<context-dir>/.
    #[arg(short, long, default_value = ".")]
    output: PathBuf,

    /// Engine / stack name used in file headers.
    #[arg(short, long, default_value = "Quartz")]
    name: String,

    /// Subdirectory name under docs/ for the context tree.
    /// Defaults to the scraped directory name, e.g. "quartz" for `--source ../quartz/src`.
    #[arg(long)]
    context_dir: Option<String>,

    /// Only write INDEX.md, vocabulary.md, and api-graph.json.
    #[arg(long)]
    minimal: bool,

    /// Print extracted items and exit without writing any files.
    #[arg(long)]
    dry_run: bool,
}

// ── serve ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
struct ServeArgs {
    /// Source directory to load. REPEATABLE — pass multiple --source flags to
    /// serve several roots from one server (e.g. quartz/src, synful_quartz/quartz/src,
    /// path_forge/src). The first source is the primary engine; items from every
    /// root are tagged with an origin slug so lookups can tell them apart.
    #[arg(short, long, default_value = "src")]
    source: Vec<PathBuf>,

    /// Engine / stack name reported in the MCP server info.
    #[arg(short, long, default_value = "Quartz")]
    name: String,
}

// ── selfcheck ────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
struct SelfcheckArgs {
    /// Source directory to validate (must contain .rs files).
    #[arg(short, long, default_value = "src")]
    source: PathBuf,

    /// Engine / stack name shown in startup recommendations.
    #[arg(short, long, default_value = "Quartz")]
    name: String,

    /// Emit machine-readable diagnostics to stdout.
    #[arg(long)]
    json: bool,
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = parse_cli_with_diagnostics()?;

    match cli.command {
        Command::Generate(args) => run_generate(args),
        Command::Serve(args)    => run_serve(args),
        Command::Selfcheck(args)=> run_selfcheck(args),
    }
}

fn parse_cli_with_diagnostics() -> Result<Cli> {
    match Cli::try_parse() {
        Ok(cli) => Ok(cli),
        Err(err) => {
            let kind = err.kind();
            let argv: Vec<String> = std::env::args().collect();
            let has_mode = argv.iter().any(|a| a == "serve" || a == "generate" || a == "selfcheck");
            let has_serve_flags = argv.iter().any(|a| a == "--source" || a == "-s" || a == "--name" || a == "-n");

            let _ = err.print();

            if !has_mode && has_serve_flags {
                eprintln!(
                    "hint: quartz-ctx requires an explicit subcommand. For MCP use:\n  quartz-ctx serve --source <path> --name <engine>"
                );
                eprintln!(
                    "hint: in .vscode/mcp.json, args should start with \"serve\" before --source/--name"
                );
            }

            let exit_code = match kind {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
                _ => 2,
            };
            std::process::exit(exit_code);
        }
    }
}

fn run_generate(args: GenerateArgs) -> Result<()> {
    eprintln!("quartz-ctx generate: scanning {}", args.source.display());

    let items = parser::parse_dir(&args.source)
        .with_context(|| format!("failed to parse source dir: {}", args.source.display()))?;

    let counts = summarise(&items);
    eprintln!(
        "  found {} items  (structs: {}  enums: {}  traits: {}  fns: {}  other: {})",
        items.len(), counts.structs, counts.enums, counts.traits, counts.fns, counts.other,
    );

    if args.dry_run {
        eprintln!("\ndry-run: listing extracted items\n");
        for item in &items {
            println!("  {:10}  {:30}  {}", item.kind.label(), item.name, item.doc_summary());
        }
        return Ok(());
    }

    if items.is_empty() {
        eprintln!("  warning: no public API items found — nothing to write.");
        return Ok(());
    }

    let ctx_dir_name = args.context_dir.unwrap_or_else(|| default_context_dir_name(&args.source));
    let ctx_dir = args.output.join("docs").join(&ctx_dir_name);

    let context = render::context::render(&items, &args.name, &ctx_dir)?;

    for (path, content) in &context.files {
        if args.minimal {
            let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or("");
            if matches!(fname, "types.md" | "traits.md" | "functions.md" | "misc.md") {
                continue;
            }
        }
        write_file(path, content.clone())?;
        eprintln!("  wrote {}", path.display());
    }

    eprintln!("\ndone. Add to your .github/copilot-instructions.md:\n");
    eprintln!("  Before writing {} code, review `docs/{}/INDEX.md`", args.name, ctx_dir_name);
    eprintln!("  and the relevant files in `docs/{}/` for available types,", ctx_dir_name);
    eprintln!("  enum variants, and API constraints.");
    eprintln!();
    eprintln!("  To also enable live skill access, add to .vscode/mcp.json:");
    eprintln!("  {{");
    eprintln!("    \"servers\": {{");
    eprintln!("      \"{}\": {{", ctx_dir_name);
    eprintln!("        \"type\": \"stdio\",");
    eprintln!("        \"command\": \"quartz-ctx\",");
    eprintln!("        \"args\": [\"serve\", \"--source\", \"{}\", \"--name\", \"{}\"]", args.source.display(), args.name);
    eprintln!("      }}");
    eprintln!("    }}");
    eprintln!("  }}");

    Ok(())
}

fn run_serve(args: ServeArgs) -> Result<()> {
    // All diagnostic output goes to stderr so stdout stays clean for JSON-RPC.
    // Build (path, origin-slug) pairs. The primary source may not be missing;
    // extra sources that are missing are skipped with a warning so one absent
    // experimental root can't take the whole server down.
    let mut sources: Vec<(PathBuf, String)> = Vec::new();
    for (i, src) in args.source.iter().enumerate() {
        if !src.exists() {
            if i == 0 {
                return Err(anyhow!(
                    "primary source path does not exist: {}\nhelp: verify --source path and MCP working directory",
                    src.display()
                ));
            }
            eprintln!("warn: skipping missing source: {}", src.display());
            continue;
        }
        let mut tag = default_context_dir_name(src);
        if sources.iter().any(|(_, t)| *t == tag) {
            // Slug collision (e.g. quartz/src and synful_quartz/quartz/src both
            // resolve to "quartz") — disambiguate with the grandparent directory.
            if let Some(alt) = src
                .components()
                .rev()
                .filter(|c| c.as_os_str() != "src")
                .nth(1)
                .map(|c| slugify(&c.as_os_str().to_string_lossy()))
                .filter(|s| !s.is_empty())
            {
                tag = alt;
            }
            if sources.iter().any(|(_, t)| *t == tag) {
                tag = format!("{tag}-{}", sources.len());
            }
        }
        sources.push((src.clone(), tag));
    }

    for (path, tag) in &sources {
        eprintln!("quartz-ctx serve: loading {} (origin: {tag})", path.display());
    }

    let items = parser::load_sources(&sources)
        .with_context(|| "failed to parse source dirs")?;

    if items.is_empty() {
        return Err(anyhow!(
            "no public API items found in any source\nhelp: verify --source points at engine src directories (example: quartz/src)"
        ));
    }

    eprintln!("  loaded {} API items from {} source(s) — listening on stdio", items.len(), sources.len());

    mcp::serve(items, &args.name, sources)
}

fn run_selfcheck(args: SelfcheckArgs) -> Result<()> {
    let source_exists = args.source.exists();
    let rs_files = if source_exists {
        WalkDir::new(&args.source)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("rs"))
            .count()
    } else {
        0
    };

    let (items_count, counts, parse_error) = if source_exists {
        match parser::parse_dir(&args.source) {
            Ok(items) => {
                let count = items.len();
                (count, Some(summarise(&items)), None)
            }
            Err(err) => (0, None, Some(err.to_string())),
        }
    } else {
        (0, None, Some("source path does not exist".to_owned()))
    };

    let ok = source_exists && rs_files > 0 && parse_error.is_none() && items_count > 0;

    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": ok,
                "source": args.source,
                "source_exists": source_exists,
                "rs_files": rs_files,
                "items": items_count,
                "counts": counts.as_ref().map(|c| json!({
                    "structs": c.structs,
                    "enums": c.enums,
                    "traits": c.traits,
                    "functions": c.fns,
                    "other": c.other,
                })),
                "error": parse_error,
                "mcp_args_recommended": ["serve", "--source", args.source.display().to_string(), "--name", args.name],
            }))?
        );
    } else {
        eprintln!("quartz-ctx selfcheck");
        eprintln!("  source: {}", args.source.display());
        eprintln!("  source exists: {}", source_exists);
        eprintln!("  rust files found: {}", rs_files);
        if let Some(c) = counts {
            eprintln!(
                "  api items: {} (structs: {} enums: {} traits: {} fns: {} other: {})",
                items_count, c.structs, c.enums, c.traits, c.fns, c.other
            );
        }
        if let Some(err) = parse_error {
            eprintln!("  parse error: {err}");
        }
        eprintln!(
            "  recommended MCP args: [\"serve\", \"--source\", \"{}\", \"--name\", \"{}\"]",
            args.source.display(),
            args.name
        );
        eprintln!("  status: {}", if ok { "OK" } else { "FAIL" });
    }

    if ok {
        Ok(())
    } else {
        Err(anyhow!("quartz-ctx selfcheck failed"))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn write_file(path: &std::path::Path, content: String) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create dir: {}", parent.display()))?;
    }
    std::fs::write(path, content)
        .with_context(|| format!("could not write: {}", path.display()))
}

struct Counts { structs: usize, enums: usize, traits: usize, fns: usize, other: usize }

fn summarise(items: &[model::ApiItem]) -> Counts {
    use model::ItemKind::*;
    let mut c = Counts { structs: 0, enums: 0, traits: 0, fns: 0, other: 0 };
    for i in items {
        match i.kind {
            Struct   => c.structs += 1,
            Enum     => c.enums   += 1,
            Trait    => c.traits  += 1,
            Function => c.fns     += 1,
            _        => c.other   += 1,
        }
    }
    c
}

fn default_context_dir_name(source: &Path) -> String {
    let candidate = if source.file_name().and_then(|name| name.to_str()) == Some("src") {
        source
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .map(|name| name.to_owned())
            .or_else(|| {
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| {
                        cwd.file_name()
                            .and_then(|name| name.to_str())
                            .map(|name| name.to_owned())
                    })
            })
    } else {
        source.file_name().and_then(|name| name.to_str()).map(|name| name.to_owned())
    };

    slugify(candidate.as_deref().unwrap_or("docs"))
}

fn slugify(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| match ch {
            'a'..='z' | '0'..='9' | '-' => ch,
            ' ' | '_' => '-',
            _ => '-',
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}
