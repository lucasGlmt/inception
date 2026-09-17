//! `InputEvent -> matching handler FunctionIds` — the boundary between
//! `inception_core::InputEvent` (driver-facing) and `lux_bytecode::EventPattern`
//! (compiler-facing).

use inception_core::InputEvent;
use lux_bytecode::{EventAction, EventBinding, EventPattern, FunctionId};

/// Matches inbound `InputEvent`s against a program's compiled event
/// table. Built fresh from a `RuntimeImage`'s `event_bindings` on every
/// load/reload (see `LoadedProgram::new`) — the runtime never re-walks
/// HIR/AST to dispatch an event.
#[derive(Debug, Clone)]
pub struct EventRouter {
    bindings: Vec<EventBinding>,
}

impl EventRouter {
    pub fn new(bindings: Vec<EventBinding>) -> Self {
        Self { bindings }
    }

    /// Every handler whose pattern matches `event`, in the bindings'
    /// declaration order (top-to-bottom `on` blocks in source) —
    /// deterministic. V1 has no priority system: every matching binding
    /// fires (see AGENTS.md/RFC 0007's "Concurrency V1" section).
    pub fn route(&self, event: InputEvent) -> Vec<FunctionId> {
        self.bindings
            .iter()
            .filter(|binding| pattern_matches(binding.pattern, event))
            .map(|binding| binding.handler)
            .collect()
    }
}

/// V1 matches on `(control, action)` only, ignoring `InputEvent::device`:
/// exactly one device kind and exactly one connected instance are
/// supported (item 14 of the task brief) — a `device` discriminant will
/// be added to `EventPattern` once a second device exists.
fn pattern_matches(pattern: EventPattern, event: InputEvent) -> bool {
    match pattern {
        EventPattern::LaunchpadPad { x, y, action } => {
            event.control == inception_core::InputControl::Pad { x, y }
                && to_core_action(action) == event.action
        }
    }
}

fn to_core_action(action: EventAction) -> inception_core::InputAction {
    match action {
        EventAction::Press => inception_core::InputAction::Press,
        EventAction::Release => inception_core::InputAction::Release,
    }
}

#[cfg(test)]
mod tests {
    use inception_core::{DeviceId, InputAction, InputControl};

    use super::*;

    const DEVICE: DeviceId = DeviceId(1);

    fn pad_binding(x: u8, y: u8, action: EventAction, handler: u32) -> EventBinding {
        EventBinding {
            pattern: EventPattern::LaunchpadPad { x, y, action },
            handler: FunctionId(handler),
        }
    }

    fn press(x: u8, y: u8) -> InputEvent {
        InputEvent {
            device: DEVICE,
            control: InputControl::Pad { x, y },
            action: InputAction::Press,
        }
    }

    fn release(x: u8, y: u8) -> InputEvent {
        InputEvent {
            device: DEVICE,
            control: InputControl::Pad { x, y },
            action: InputAction::Release,
        }
    }

    #[test]
    fn routes_a_press_to_its_own_handler_only() {
        let router = EventRouter::new(vec![
            pad_binding(1, 1, EventAction::Press, 0),
            pad_binding(1, 2, EventAction::Press, 1),
        ]);

        assert_eq!(router.route(press(1, 1)), vec![FunctionId(0)]);
    }

    #[test]
    fn a_different_pad_does_not_trigger_an_unrelated_handler() {
        let router = EventRouter::new(vec![pad_binding(1, 1, EventAction::Press, 0)]);

        assert_eq!(router.route(press(1, 2)), Vec::<FunctionId>::new());
    }

    #[test]
    fn release_routes_to_the_release_handler_not_the_press_handler() {
        let router = EventRouter::new(vec![
            pad_binding(1, 1, EventAction::Press, 0),
            pad_binding(1, 1, EventAction::Release, 1),
        ]);

        assert_eq!(router.route(release(1, 1)), vec![FunctionId(1)]);
    }

    #[test]
    fn two_bindings_matching_the_same_event_both_fire_in_declaration_order() {
        let router = EventRouter::new(vec![
            pad_binding(1, 1, EventAction::Press, 0),
            pad_binding(1, 1, EventAction::Press, 1),
        ]);

        assert_eq!(
            router.route(press(1, 1)),
            vec![FunctionId(0), FunctionId(1)]
        );
    }
}
