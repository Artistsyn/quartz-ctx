/// Usage patterns extracted from api.txt
/// 
/// Real-world examples of how to use Quartz APIs correctly.
/// Each pattern includes: name, category, description, working code example, and context.

#[derive(Clone, Debug)]
pub struct UsagePattern {
    pub name: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub code: &'static str,
    pub context: &'static str,
    pub source_reference: &'static str,
}

pub fn get_usage_patterns() -> Vec<UsagePattern> {
    vec![
        // ── TEXT RENDERING ──
        UsagePattern {
            name: "multi-span colored text",
            category: "text",
            description: "Create text with different colors and sizes on same line",
            code: r#"
let text = Text::new(
    vec![
        Span::new(
            "fn ".to_string(),
            14.0,              // font size
            Some(20.0),        // line height
            font.clone(),
            Color(255, 157, 0, 255),  // orange
            0.0,               // offset
        ),
        Span::new(
            "main".to_string(),
            14.0,
            Some(20.0),
            font.clone(),
            Color(255, 255, 255, 255),  // white
            0.0,
        ),
        Span::new(
            "()".to_string(),
            14.0,
            Some(20.0),
            font.clone(),
            Color(255, 255, 255, 255),  // white
            0.0,
        ),
    ],
    None,        // no width → single line
    Align::Left,
    None,        // no height
);
            "#,
            context: "Each Span can have different color, size, font, offset. Renders inline on same line. Use for syntax highlighting, mixed-font labels.",
            source_reference: "api.txt lines 130-138",
        },
        UsagePattern {
            name: "word-wrapped text layout",
            category: "text",
            description: "Enable word-wrapping by specifying a width constraint",
            code: r#"
let text = Text::new(
    vec![
        Span::new(
            "This is a long paragraph that will wrap to fit within the specified width.".to_string(),
            14.0,
            Some(20.0),
            font.clone(),
            Color(255, 255, 255, 255),
            0.0,
        ),
    ],
    Some(200.0),  // width constraint → enable word-wrap
    Align::Left,
    Some(400.0),  // max height (optional)
);
            "#,
            context: "Newlines (\\n) start new lines. Width enables word-wrap. Line height controls vertical spacing (typically 1.35x font size for body, 1.55x for code).",
            source_reference: "api.txt lines 143-156",
        },
        UsagePattern {
            name: "font size scaling from logical to virtual pixels",
            category: "text",
            description: "Author font sizes in logical screen pixels; engine handles virtual canvas conversion",
            code: r#"
// Author's perspective: 42px on a 1920x1080 screen
let score_text = c.make_text(
    format!("SCORE: {}", score),
    42.0,  // logical screen pixels
    Color(255, 255, 255, 255),
    Align::Right,
    hud_font.clone(),
);

// If you need to compute virtual size manually:
let virtual_size = c.virt_font_size(42.0);  // converts 42px → virtual units
            "#,
            context: "make_text() internally calls virtual_scale() and converts logical sizes to virtual canvas pixels. Use this pattern for HUD elements authored in screen space.",
            source_reference: "api.txt lines 385-397",
        },
        // ── OBJECT POOLING ──
        UsagePattern {
            name: "object pool lifecycle",
            category: "pooling",
            description: "Pre-allocate, acquire, use, release pattern for bullets/particles",
            code: r#"
// Setup (once, on scene init)
let bullet_template = GameObject::build("bullet")
    .drawable(sprite)
    .finish();
cv.create_pool("bullets", bullet_template, 100);  // Pre-allocate 100

// Usage loop in on_update:
if let Some(bullet_name) = cv.pool_acquire("bullets", player_pos) {
    // Use bullet: add_target, add_action, etc.
    cv.add_action(
        &bullet_name,
        Action::SetVelocity(direction.0 * 500.0, direction.1 * 500.0),
    );
    // Later, when bullet expires:
    cv.pool_release(&bullet_name);
}

// Check availability
let available = cv.pool_available("bullets");  // How many unused
let active = cv.pool_active("bullets");        // How many in use
            "#,
            context: "Reduces allocation overhead. Pre-spawn objects instead of creating/destroying each cycle. pool_acquire resets ONLY position and momentum—manually reset other properties (rotation, color, animation).",
            source_reference: "api.txt lines 454-474",
        },
        // ── INPUT HANDLING ──
        UsagePattern {
            name: "check modifiers in on_key_press",
            category: "input",
            description: "Read modifier state (Ctrl/Shift/Alt) inside on_key_press callback",
            code: r#"
cv.on_key_press(move |cv, key| {
    let ctrl  = cv.is_key_held(&Key::Named(NamedKey::Control));
    let shift = cv.is_key_held(&Key::Named(NamedKey::Shift));
    let alt   = cv.is_key_held(&Key::Named(NamedKey::Alt));
    
    if ctrl {
        // Handle Ctrl+key combos
        match key {
            Key::Character('s') => println!("Ctrl+S: Save"),
            Key::Character('c') => println!("Ctrl+C: Copy"),
            _ => {}
        }
        return;
    }
    
    if shift {
        match key {
            Key::Character('?') => println!("Shift+?: Help"),
            _ => {}
        }
        return;
    }
    
    // Handle bare keys (no modifiers)
    match key {
        Key::Character('q') => println!("Q: Quit"),
        Key::Named(NamedKey::Escape) => println!("Escape pressed"),
        _ => {}
    }
});
            "#,
            context: "Modifiers are tracked but don't fire events alone. Always check modifiers in your on_key_press callback. For complex input, use GameEvent with modifier filters.",
            source_reference: "api.txt lines 321-327",
        },
        // ── HOT RELOAD ──
        UsagePattern {
            name: "hot-reload typed config with Shared<T>",
            category: "file_watching",
            description: "Watch and auto-parse config file using FromSource trait",
            code: r#"
use std::sync::Arc;

// Define config type
#[derive(Clone, Debug)]
struct AppSettings {
    difficulty: u32,
    music_volume: f32,
}

impl FromSource for AppSettings {
    fn from_source(text: &str) -> Result<Self> {
        // Parse text → AppSettings
        let mut difficulty = 1;
        let mut music_volume = 1.0;
        
        for line in text.lines() {
            if let Some(val) = line.strip_prefix("difficulty:") {
                difficulty = val.trim().parse().unwrap_or(1);
            }
            if let Some(val) = line.strip_prefix("music_volume:") {
                music_volume = val.trim().parse().unwrap_or(1.0);
            }
        }
        
        Ok(AppSettings { difficulty, music_volume })
    }
}

// In game init:
let settings = Shared::new(AppSettings::default());
cv.watch_source("config.txt", settings.clone());

// In on_update, check for changes:
cv.on_update(move |c| {
    if settings.changed() {
        let new_settings = settings.get();
        println!("Config reloaded: difficulty={}", new_settings.difficulty);
        // Rebuild UI, adjust game parameters, etc.
    }
});
            "#,
            context: "Poll interval 0.5s. Use .changed() to detect file updates. Implement FromSource for custom config parsing. File changes have up to 0.5s latency.",
            source_reference: "api.txt lines 444-450",
        },
        // ── COLLISION ──
        UsagePattern {
            name: "filter collisions by layer bitmask",
            category: "collision",
            description: "Use collision layers to control which objects collide with each other",
            code: r#"
// Define layers (bits 0-31 available)
const LAYER_PLAYER: u32 = 1 << 0;    // Bit 0
const LAYER_ENEMY: u32 = 1 << 1;     // Bit 1
const LAYER_WALL: u32 = 1 << 2;      // Bit 2
const LAYER_TRIGGER: u32 = 1 << 3;   // Bit 3

// Create player: collides with walls and enemies
cv.add_action(
    "player",
    Action::SetCollisionLayer(LAYER_PLAYER, LAYER_WALL | LAYER_ENEMY),
);

// Create wall: collides with everything
cv.add_action(
    "wall",
    Action::SetCollisionLayer(LAYER_WALL, LAYER_PLAYER | LAYER_ENEMY | LAYER_TRIGGER),
);

// Create trigger: only collides with player
cv.add_action(
    "trigger",
    Action::SetCollisionLayer(LAYER_TRIGGER, LAYER_PLAYER),
);

// Query collisions: does player collide with walls?
if cv.collision_between(&Target::name("player"), &Target::layer(LAYER_WALL)) {
    println!("Player hit a wall");
}
            "#,
            context: "First u32 = self layer(s). Second u32 = bitmask of layers to collide with. Use to prevent unwanted collisions (e.g., triggers shouldn't block player).",
            source_reference: "api.txt collision layer section",
        },
    ]
}

pub fn get_pattern(name: &str) -> Option<UsagePattern> {
    get_usage_patterns()
        .into_iter()
        .find(|p| p.name.to_lowercase() == name.to_lowercase())
}

pub fn get_patterns_by_category(category: &str) -> Vec<UsagePattern> {
    get_usage_patterns()
        .into_iter()
        .filter(|p| p.category.to_lowercase() == category.to_lowercase())
        .collect()
}
