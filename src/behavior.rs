/// Behavioral rules extracted from api.txt
/// 
/// These rules describe how the engine actually behaves at runtime.
/// They are not visible in type signatures but are critical for correct usage.
///
/// Examples: when events fire, order of execution, edge cases.

#[derive(Clone, Debug)]
pub struct BehaviorRule {
    pub category: &'static str,
    pub rule: &'static str,
    pub when_applies: &'static str,
    pub examples: Vec<&'static str>,
    pub consequence: &'static str,
    pub source_reference: &'static str,
}

pub fn get_behavior_rules() -> Vec<BehaviorRule> {
    vec![
        // ── INPUT SYSTEM ──
        BehaviorRule {
            category: "input",
            rule: "KeyHold events only fire when NO modifiers are held",
            when_applies: "During tick step 2 (held-key events processing)",
            examples: vec![
                "Key A held alone → KeyHold fires",
                "Key A + Shift held → KeyHold does NOT fire",
                "Key A + Shift held, but GameEvent specifies modifier requirement → fires if matched",
            ],
            consequence: "Modifier-only presses never trigger KeyHold. Always check modifiers in on_key_press callback.",
            source_reference: "api.txt lines 319-338",
        },
        BehaviorRule {
            category: "input",
            rule: "on_key_press fires BEFORE pause gate (fires even when paused)",
            when_applies: "During pause() state",
            examples: vec![
                "Game paused, Escape pressed → on_key_press fires → can check and toggle pause",
                "Game paused, other GameEvents do NOT fire",
                "Game paused, normal object physics do NOT run",
            ],
            consequence: "Use on_key_press to implement pause toggle. Use GameEvents for pause-aware input.",
            source_reference: "api.txt user guide",
        },
        BehaviorRule {
            category: "input",
            rule: "Modifier keys (Ctrl, Shift, Alt, Meta, CapsLock) tracked but never fire events alone",
            when_applies: "Whenever checking held keys",
            examples: vec![
                "Pressing Shift alone: is_key_held(Shift) = true, but on_key_press never fires",
                "Pressing A: on_key_press(A) fires; inside callback, check is_key_held(Shift) for Shift+A",
            ],
            consequence: "Always check modifier state inside on_key_press callbacks. Never expect on_key_press for bare modifiers.",
            source_reference: "api.txt lines 319-328",
        },
        // ── PHYSICS & TIMING ──
        BehaviorRule {
            category: "physics",
            rule: "Physics step (step 8) runs BEFORE camera transform (step 11)",
            when_applies: "Each frame tick",
            examples: vec![
                "on_update modifies position → physics resolves collisions → camera reads final position",
                "Reading velocity inside on_update returns last-frame velocity; step 8 updates it",
                "Camera.follow() reads final post-physics positions (step 11)",
            ],
            consequence: "Physics changes are visible to camera. Don't try to compensate for stale velocity before step 8.",
            source_reference: "api.txt lines 266-287 (tick loop order)",
        },
        BehaviorRule {
            category: "physics",
            rule: "Boundary collision events (step 13) fire LAST after all state changes",
            when_applies: "End of tick loop",
            examples: vec![
                "Object exits map → collision event fires → can queue removal or wrapping",
                "Perfect place for cleanup logic that shouldn't interfere with same-frame physics",
            ],
            consequence: "Use boundary events for cleanup/wrapping. Earlier steps let you modify state before boundary check.",
            source_reference: "api.txt lines 266-287 (tick loop step 13)",
        },
        // ── RENDERING & CAMERA ──
        BehaviorRule {
            category: "rendering",
            rule: "Flash overlay is auto-synced during camera transform (step 11)",
            when_applies: "When camera_flash() or Action::Flash is used",
            examples: vec![
                "__quartz_flash_overlay GameObject is managed by engine",
                "Manual overlay setup is unnecessary (engine handles it)",
            ],
            consequence: "Don't manually create or modify __quartz_flash_overlay. Use camera_flash() or Action::Flash.",
            source_reference: "api.txt camera effects section",
        },
        BehaviorRule {
            category: "rendering",
            rule: "Large objects (30000+ width) get uniform tinting from center point",
            when_applies: "When lighting is applied to large objects",
            examples: vec![
                "30000-wide background with add_light() → entire object gets same color from center",
                "30000-wide background with .unlit() → always at full brightness (correct for huge objects)",
            ],
            consequence: "Keep large decorative backgrounds .unlit(). Use smaller lit objects for lighting variation.",
            source_reference: "api.txt lines 1624-1626",
        },
        // ── HOT RELOAD ──
        BehaviorRule {
            category: "file_watching",
            rule: "Hot-reload file poll interval is 0.5 seconds",
            when_applies: "During watch_file() or watch_source() callbacks",
            examples: vec![
                "File modified at T=0.1s. Detected at T=0.5s (0.4s latency).",
                "If testing, account for up to 0.5s delay for file changes to take effect.",
            ],
            consequence: "Don't expect immediate hot-reload. Design tests/demos to account for 0.5s latency.",
            source_reference: "api.txt lines 432-434",
        },
        // ── TEXT RENDERING ──
        BehaviorRule {
            category: "text",
            rule: "Text layout is cached by hash; identical text reuses cached layout",
            when_applies: "Rendering Text objects",
            examples: vec![
                "Two Span with same content/font/size → layout computed once, reused",
                "Changing color alone → layout unchanged, rendering fast",
            ],
            consequence: "Changing layout parameters is cheaper than changing color. Design accordingly for performance.",
            source_reference: "api.txt text rendering section",
        },
    ]
}

pub fn get_behavior_rule(query: &str) -> Vec<BehaviorRule> {
    get_behavior_rules()
        .into_iter()
        .filter(|r| {
            r.category.contains(query)
                || r.rule.to_lowercase().contains(&query.to_lowercase())
                || r.examples.iter().any(|e| e.to_lowercase().contains(&query.to_lowercase()))
        })
        .collect()
}
