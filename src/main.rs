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

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

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
    /// Source directory to load (scanned once at startup).
    #[arg(short, long, default_value = "src")]
    source: PathBuf,

    /// Engine / stack name reported in the MCP server info.
    #[arg(short, long, default_value = "Quartz")]
    name: String,
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Generate(args) => run_generate(args),
        Command::Serve(args)    => run_serve(args),
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
    eprintln!("quartz-ctx serve: loading {}", args.source.display());

    let items = parser::parse_dir(&args.source)
        .with_context(|| format!("failed to parse source dir: {}", args.source.display()))?;

    eprintln!("  loaded {} API items — listening on stdio", items.len());

    mcp::serve(items, &args.name)
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
