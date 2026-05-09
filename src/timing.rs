/// Tick loop execution order extraction from api.txt
/// 
/// Each frame executes in this fixed order. Understanding the sequence is critical
/// for timing behavior, physics, events, and camera updates.
///
/// Source: api.txt lines 266-287

#[derive(Clone, Debug)]
pub struct TickLoopStep {
    pub step: usize,
    pub name: &'static str,
    pub description: &'static str,
    pub preconditions: Vec<&'static str>,
    pub effects: Vec<&'static str>,
    pub critical_note: Option<&'static str>,
}

/// Get the complete tick loop execution order
pub fn get_tick_loop_order() -> Vec<TickLoopStep> {
    vec![
        TickLoopStep {
            step: 1,
            name: "on_update callbacks",
            description: "Runs Canvas::on_update callbacks registered by user code",
            preconditions: vec![],
            effects: vec!["Game state can be modified", "Objects can be created/destroyed"],
            critical_note: Some("Happens first each frame. State modified here is seen by physics step."),
        },
        TickLoopStep {
            step: 2,
            name: "held-key events",
            description: "Process keys held from previous frame (KeyHold GameEvents)",
            preconditions: vec!["on_update complete"],
            effects: vec!["KeyHold GameEvents may fire for keys held without modifiers"],
            critical_note: Some("Only fires when NO modifiers are held. Ctrl/Shift/Alt alone never fire."),
        },
        TickLoopStep {
            step: 3,
            name: "all Tick GameEvents",
            description: "GameEvents with target_type filtered to all matched objects",
            preconditions: vec!["held-key events complete"],
            effects: vec!["Tick GameEvents fire for matched objects"],
            critical_note: None,
        },
        TickLoopStep {
            step: 4,
            name: "mouse-over events",
            description: "Mouse position queries and mouse-over callbacks fire",
            preconditions: vec!["Tick GameEvents complete"],
            effects: vec!["on_mouse_over, is_point_inside() calls work", "Camera transform not yet applied to world coords"],
            critical_note: None,
        },
        TickLoopStep {
            step: 5,
            name: "custom GameEvents",
            description: "All user-triggered GameEvents not handled above",
            preconditions: vec!["mouse-over events complete"],
            effects: vec!["Custom GameEvents fire for matched objects"],
            critical_note: None,
        },
        TickLoopStep {
            step: 6,
            name: "hot-reload poll",
            description: "Check watched files for changes (0.5s poll interval)",
            preconditions: vec!["custom GameEvents complete"],
            effects: vec!["watch_file callbacks may fire", "New source parsed if modified"],
            critical_note: Some("Only fires every 0.5s. Latency up to 0.5s for file change detection."),
        },
        TickLoopStep {
            step: 7,
            name: "object update loop",
            description: "Every object's internal update handler runs",
            preconditions: vec!["hot-reload complete"],
            effects: vec!["Object animations advance", "Sprite frames update", "Position/momentum read"],
            critical_note: None,
        },
        TickLoopStep {
            step: 8,
            name: "physics step",
            description: "8a. Crystalline solver (if enabled) OR 8b. legacy collision resolution (otherwise)",
            preconditions: vec!["object update loop complete"],
            effects: vec!["Velocities applied", "Collisions resolved", "Positions updated", "Momentum/velocity now reflect resolved state"],
            critical_note: Some("CRITICAL: Physics runs here. Velocity values from step 7 are now final."),
        },
        TickLoopStep {
            step: 9,
            name: "planet landings",
            description: "Auto-detect objects on planetary surfaces and apply landing behavior",
            preconditions: vec!["physics step complete"],
            effects: vec!["Landing callbacks fire", "Grounded state set"],
            critical_note: None,
        },
        TickLoopStep {
            step: 10,
            name: "auto-align",
            description: "Auto-align gravity if planet system is active",
            preconditions: vec!["planet landings complete"],
            effects: vec!["Object rotations may change to align with planet gravity"],
            critical_note: None,
        },
        TickLoopStep {
            step: 11,
            name: "camera transform",
            description: "Camera position/zoom/effects applied, flash overlay auto-synced",
            preconditions: vec!["auto-align complete"],
            effects: vec!["World coordinates converted to screen coordinates", "Flash overlay updated"],
            critical_note: Some("CRITICAL: Camera reads final positions from physics step. Rendering uses camera-transformed coords. Flash overlay synced here."),
        },
        TickLoopStep {
            step: 12,
            name: "sorted-offset sync",
            description: "Synchronize sorted-offset layer rendering data",
            preconditions: vec!["camera transform complete"],
            effects: vec!["Depth sorting finalized", "Render queue built"],
            critical_note: None,
        },
        TickLoopStep {
            step: 13,
            name: "boundary collision events",
            description: "Boundary collision events fire for objects that exited/entered map edges",
            preconditions: vec!["sorted-offset sync complete"],
            effects: vec!["Boundary collision GameEvents fire", "Can trigger object removal, wrapping, etc."],
            critical_note: Some("Fires LAST. After all state changes. Good place for cleanup."),
        },
    ]
}

/// Get timing constants used throughout the engine
pub struct TimingConstants {
    pub tick_delta: f32,
    pub hot_reload_poll_interval: f32,
}

pub fn get_timing_constants() -> TimingConstants {
    TimingConstants {
        tick_delta: 0.016,  // 60 Hz fixed
        hot_reload_poll_interval: 0.5,  // File change detection latency
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_loop_order_complete() {
        let order = get_tick_loop_order();
        assert_eq!(order.len(), 13, "Tick loop should have exactly 13 steps");
        
        for (i, step) in order.iter().enumerate() {
            assert_eq!(step.step, i + 1, "Step numbers must be sequential");
        }
    }
}
