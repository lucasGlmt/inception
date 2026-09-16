//! Runtime orchestration over an already linked [`RuntimeImage`].
//!
//! Patch validation, role resolution, and textual lookup belong to the linker;
//! this crate only owns numeric runtime structures.

use std::collections::HashMap;

use inception_core::{Clock, LightingState, Timestamp, TransitionEngine, UniverseId};
use inception_linker::RuntimeImage;
use inception_renderer::{ResolvedRig, UniverseFrame};
use inception_vm::{Vm, VmError, VmInitError};

#[derive(Debug)]
pub struct Runtime {
    vm: Vm,
    lighting: LightingState,
    transitions: TransitionEngine,
    rig: ResolvedRig,
}

impl Runtime {
    pub fn new(image: RuntimeImage) -> Result<Self, VmInitError> {
        let lighting = image.lighting_state();
        Ok(Self {
            vm: Vm::new(image.bytecode)?,
            lighting,
            transitions: TransitionEngine::new(),
            rig: image.rig,
        })
    }

    pub fn start(&mut self) -> Result<(), VmError> {
        self.vm.start()
    }

    pub fn run_until_blocked<C: Clock>(&mut self, clock: &C) -> Result<(), VmError> {
        self.vm
            .run_until_blocked(clock, &mut self.lighting, &mut self.transitions)
    }

    /// Samples absolute-time transitions, then renders the resulting semantic
    /// state. No names, patch objects, or fixture definitions are consulted.
    pub fn render_at(
        &mut self,
        now: Timestamp,
        frames: &mut HashMap<UniverseId, UniverseFrame>,
    ) -> Result<(), inception_core::TransitionError> {
        self.transitions.sample(now, &mut self.lighting)?;
        inception_renderer::render(&self.lighting, &self.rig, frames);
        Ok(())
    }

    pub fn active_transition_count(&self) -> usize {
        self.transitions.active_count()
    }
}
