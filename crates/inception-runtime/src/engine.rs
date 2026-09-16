use std::collections::{BTreeMap, BTreeSet, HashMap};

use inception_core::{
    AttributeValue, Clock, LightingState, Timestamp, TransitionEngine, UniverseId,
};
use inception_driver_dmx::DmxOutput;
use inception_linker::RuntimeImage;
use inception_renderer::{ResolvedFixture, ResolvedRig, UniverseFrame};
use inception_vm::{Vm, VmInitError};

use crate::{OutputOperation, RuntimeError};

#[derive(Debug, Clone, Copy)]
struct TickClock(Timestamp);

impl Clock for TickClock {
    fn now(&self) -> Timestamp {
        self.0
    }
}

/// All state owned by one linked build and replaced on reload.
#[derive(Debug)]
pub struct LoadedProgram {
    vm: Vm,
    lighting: LightingState,
    transitions: TransitionEngine,
    rig: ResolvedRig,
    fixture_keys: Vec<String>,
}

impl LoadedProgram {
    pub fn new(image: RuntimeImage) -> Result<Self, VmInitError> {
        let lighting = image.lighting_state();
        Ok(Self {
            vm: Vm::new(image.bytecode)?,
            lighting,
            transitions: TransitionEngine::new(),
            rig: image.rig,
            fixture_keys: image.fixture_keys,
        })
    }

    fn start(&mut self) -> Result<(), inception_vm::VmError> {
        self.vm.start()
    }

    fn advance(&mut self, now: Timestamp) -> Result<(), RuntimeError<std::convert::Infallible>> {
        self.vm
            .run_until_blocked(&TickClock(now), &mut self.lighting, &mut self.transitions)
            .map_err(RuntimeError::Vm)?;
        self.transitions
            .sample(now, &mut self.lighting)
            .map_err(RuntimeError::Transition)?;
        // Signal bindings are resampled every frame regardless of whether
        // the VM produced any progress this tick (it may be waiting, or
        // already finished) — a binding outlives the instruction that
        // created it, see `Vm::sample_signal_bindings`'s docs.
        self.vm
            .sample_signal_bindings(now, &mut self.lighting)
            .map_err(RuntimeError::Signal)
    }

    /// Freezes this program's effective lighting state at `now` before it
    /// is discarded by a hot reload — transitions and, per the same rule,
    /// active signal bindings, so `preserve_compatible_state_from` reads a
    /// fresh value rather than whatever `LightingState` last happened to
    /// hold (see `Vm::sample_signal_bindings`'s docs; the new program's
    /// own `<-` statements will recreate their own bindings on their own
    /// `SignalStore` when it runs — nothing here migrates the old one).
    fn sample(&mut self, now: Timestamp) -> Result<(), RuntimeError<std::convert::Infallible>> {
        self.transitions
            .sample(now, &mut self.lighting)
            .map_err(RuntimeError::Transition)?;
        self.vm
            .sample_signal_bindings(now, &mut self.lighting)
            .map_err(RuntimeError::Signal)
    }

    fn fixture_by_key(&self) -> BTreeMap<String, ResolvedFixture> {
        self.fixture_keys
            .iter()
            .zip(self.rig.fixtures.iter().copied())
            .map(|(key, fixture)| (key.clone(), fixture))
            .collect()
    }

    fn preserve_compatible_state_from(&mut self, old: &LoadedProgram) -> usize {
        let old_fixtures = old.fixture_by_key();
        let mut preserved = 0;
        for (key, new_fixture) in self.fixture_by_key() {
            let Some(old_fixture) = old_fixtures.get(&key).copied() else {
                continue;
            };
            let mut compatible = false;
            if old_fixture.intensity.is_some() && new_fixture.intensity.is_some() {
                self.lighting.set_fixture_attribute(
                    new_fixture.id,
                    AttributeValue::Intensity(old.lighting.intensity(old_fixture.id)),
                );
                compatible = true;
            }
            if old_fixture.color.is_some() && new_fixture.color.is_some() {
                self.lighting.set_fixture_attribute(
                    new_fixture.id,
                    AttributeValue::Color(old.lighting.color(old_fixture.id)),
                );
                compatible = true;
            }
            if compatible {
                preserved += 1;
            }
        }
        preserved
    }

    pub fn active_transition_count(&self) -> usize {
        self.transitions.active_count()
    }

    pub fn active_binding_count(&self) -> usize {
        self.vm.bindings().active_count()
    }

    pub fn lighting_state(&self) -> &LightingState {
        &self.lighting
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReloadReport {
    pub preserved_fixtures: usize,
}

/// Long-lived owner of the output connection and counters. Only its
/// `LoadedProgram` is replaced by a hot reload.
#[derive(Debug)]
pub struct RuntimeHost<O> {
    program: LoadedProgram,
    output: O,
    universes: BTreeSet<UniverseId>,
    frames: HashMap<UniverseId, UniverseFrame>,
    frames_sent: u64,
}

/// Backwards-compatible name for the original runtime API.
pub type RuntimeEngine<O> = RuntimeHost<O>;

impl<O: DmxOutput> RuntimeHost<O> {
    pub fn new(image: RuntimeImage, output: O) -> Result<Self, VmInitError> {
        Ok(Self::from_program(LoadedProgram::new(image)?, output))
    }

    pub fn from_program(program: LoadedProgram, output: O) -> Self {
        let universes = active_universes(&program.rig).into_iter().collect();
        Self {
            program,
            output,
            universes,
            frames: HashMap::new(),
            frames_sent: 0,
        }
    }

    pub fn start(&mut self, now: Timestamp) -> Result<(), RuntimeError<O::Error>> {
        self.program.start().map_err(RuntimeError::Vm)?;
        self.tick(now)
    }

    pub fn tick(&mut self, now: Timestamp) -> Result<(), RuntimeError<O::Error>> {
        self.program.advance(now).map_err(|error| match error {
            RuntimeError::Vm(error) => RuntimeError::Vm(error),
            RuntimeError::Transition(error) => RuntimeError::Transition(error),
            RuntimeError::Signal(error) => RuntimeError::Signal(error),
            RuntimeError::DmxOutput { source, .. } => match source {},
        })?;
        self.render_and_send()
    }

    /// Candidate construction and validation occur before this short swap.
    /// The old effective state is sampled at `now`; compatible fixtures are
    /// copied by stable patch name, while VM frames and transitions reset.
    pub fn reload(
        &mut self,
        mut candidate: LoadedProgram,
        now: Timestamp,
    ) -> Result<ReloadReport, RuntimeError<O::Error>> {
        candidate.start().map_err(RuntimeError::Vm)?;
        self.program.sample(now).map_err(|error| match error {
            RuntimeError::Vm(error) => RuntimeError::Vm(error),
            RuntimeError::Transition(error) => RuntimeError::Transition(error),
            RuntimeError::Signal(error) => RuntimeError::Signal(error),
            RuntimeError::DmxOutput { source, .. } => match source {},
        })?;
        let preserved_fixtures = candidate.preserve_compatible_state_from(&self.program);
        self.universes.extend(active_universes(&candidate.rig));
        self.program = candidate;
        Ok(ReloadReport { preserved_fixtures })
    }

    fn render_and_send(&mut self) -> Result<(), RuntimeError<O::Error>> {
        // Black first so removed fixtures/channels cannot retain stale DMX.
        for &universe in &self.universes {
            self.frames.insert(universe, UniverseFrame::black());
        }
        inception_renderer::render(&self.program.lighting, &self.program.rig, &mut self.frames);
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

    /// Immediate blackout without closing the long-lived driver.
    pub fn blackout(&mut self) -> Result<(), RuntimeError<O::Error>> {
        let black = UniverseFrame::black();
        for &universe in &self.universes {
            self.output
                .send(universe, &black)
                .map_err(|source| RuntimeError::DmxOutput {
                    operation: OutputOperation::Blackout(universe),
                    source,
                })?;
            self.frames_sent = self.frames_sent.saturating_add(1);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), RuntimeError<O::Error>> {
        let blackout_result = self.blackout();
        let close_result = self
            .output
            .close()
            .map_err(|source| RuntimeError::DmxOutput {
                operation: OutputOperation::Close,
                source,
            });
        blackout_result.and(close_result)
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
        self.program.active_transition_count()
    }

    pub fn active_binding_count(&self) -> usize {
        self.program.active_binding_count()
    }

    pub fn universes(&self) -> Vec<UniverseId> {
        self.universes.iter().copied().collect()
    }

    pub fn program(&self) -> &LoadedProgram {
        &self.program
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
