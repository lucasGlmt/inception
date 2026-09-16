use std::collections::{BTreeSet, HashMap};

use inception_core::{Clock, LightingState, Timestamp, TransitionEngine, UniverseId};
use inception_driver_dmx::DmxOutput;
use inception_linker::RuntimeImage;
use inception_renderer::{ResolvedRig, UniverseFrame};
use inception_vm::{Vm, VmInitError};

use crate::{OutputOperation, RuntimeError};

#[derive(Debug, Clone, Copy)]
struct TickClock(Timestamp);

impl Clock for TickClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

#[derive(Debug)]
pub struct RuntimeEngine<O> {
    vm: Vm,
    lighting: LightingState,
    transitions: TransitionEngine,
    rig: ResolvedRig,
    universes: Vec<UniverseId>,
    frames: HashMap<UniverseId, UniverseFrame>,
    output: O,
    frames_sent: u64,
}

impl<O: DmxOutput> RuntimeEngine<O> {
    pub fn new(image: RuntimeImage, output: O) -> Result<Self, VmInitError> {
        let universes = active_universes(&image.rig);
        let frames = universes
            .iter()
            .copied()
            .map(|universe| (universe, UniverseFrame::black()))
            .collect();
        let lighting = image.lighting_state();
        Ok(Self {
            vm: Vm::new(image.bytecode)?,
            lighting,
            transitions: TransitionEngine::new(),
            rig: image.rig,
            universes,
            frames,
            output,
            frames_sent: 0,
        })
    }

    /// Starts the program and explicitly sends its initial rendered state.
    pub fn start(&mut self, now: Timestamp) -> Result<(), RuntimeError<O::Error>> {
        self.vm.start().map_err(RuntimeError::Vm)?;
        self.tick(now)
    }

    /// Executes one logical cycle at exactly `now`. The same timestamp is
    /// supplied to the VM and transition sampler, so a cycle cannot observe
    /// two subtly different instants.
    pub fn tick(&mut self, now: Timestamp) -> Result<(), RuntimeError<O::Error>> {
        self.vm
            .run_until_blocked(&TickClock(now), &mut self.lighting, &mut self.transitions)
            .map_err(RuntimeError::Vm)?;
        self.transitions
            .sample(now, &mut self.lighting)
            .map_err(RuntimeError::Transition)?;
        inception_renderer::render(&self.lighting, &self.rig, &mut self.frames);

        for &universe in &self.universes {
            self.output
                .send(universe, &self.frames[&universe])
                .map_err(|source| RuntimeError::DmxOutput {
                    operation: OutputOperation::Send(universe),
                    source,
                })?;
            self.frames_sent = self.frames_sent.saturating_add(1);
        }
        Ok(())
    }

    /// Sends one final blackout per active universe, then closes the output.
    /// Closing is attempted even when a blackout write fails.
    pub fn stop(&mut self) -> Result<(), RuntimeError<O::Error>> {
        let black = UniverseFrame::black();
        let mut send_error = None;
        for &universe in &self.universes {
            if let Err(source) = self.output.send(universe, &black) {
                if send_error.is_none() {
                    send_error = Some(RuntimeError::DmxOutput {
                        operation: OutputOperation::Blackout(universe),
                        source,
                    });
                }
            } else {
                self.frames_sent = self.frames_sent.saturating_add(1);
            }
        }
        let close_result = self
            .output
            .close()
            .map_err(|source| RuntimeError::DmxOutput {
                operation: OutputOperation::Close,
                source,
            });
        match (send_error, close_result) {
            (Some(error), _) => Err(error),
            (None, result) => result,
        }
    }

    pub fn output(&self) -> &O {
        &self.output
    }

    pub fn output_mut(&mut self) -> &mut O {
        &mut self.output
    }

    pub fn into_output(self) -> O {
        self.output
    }

    pub const fn frames_sent(&self) -> u64 {
        self.frames_sent
    }

    pub fn active_transition_count(&self) -> usize {
        self.transitions.active_count()
    }

    pub fn universes(&self) -> &[UniverseId] {
        &self.universes
    }
}

fn active_universes(rig: &ResolvedRig) -> Vec<UniverseId> {
    let mut universes = BTreeSet::new();
    for fixture in &rig.fixtures {
        if let Some(mapping) = fixture.intensity {
            universes.insert(mapping.universe);
        }
        if let Some(mapping) = fixture.color {
            universes.insert(mapping.universe);
        }
    }
    universes.into_iter().collect()
}
