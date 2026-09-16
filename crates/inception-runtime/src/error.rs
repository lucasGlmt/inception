use inception_core::{TransitionError, UniverseId};
use inception_vm::{SignalError, VmError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputOperation {
    Send(UniverseId),
    Blackout(UniverseId),
    Close,
}

#[derive(Debug)]
pub enum RuntimeError<E> {
    Vm(VmError),
    Transition(TransitionError),
    Signal(SignalError),
    DmxOutput {
        operation: OutputOperation,
        source: E,
    },
}

impl<E: std::fmt::Display> std::fmt::Display for RuntimeError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vm(error) => write!(formatter, "VM execution failed: {error:?}"),
            Self::Transition(error) => {
                write!(formatter, "transition sampling failed: {error:?}")
            }
            Self::Signal(error) => {
                write!(formatter, "signal binding sampling failed: {error:?}")
            }
            Self::DmxOutput { operation, source } => {
                write!(
                    formatter,
                    "DMX output failed during {operation:?}: {source}"
                )
            }
        }
    }
}

impl<E: std::fmt::Debug + std::fmt::Display> std::error::Error for RuntimeError<E> {}

#[derive(Debug)]
pub enum RuntimeLoopError<OutputError, SleepError> {
    Engine(RuntimeError<OutputError>),
    Sleep(SleepError),
    NotStarted,
}

impl<OutputError: std::fmt::Display, SleepError: std::fmt::Display> std::fmt::Display
    for RuntimeLoopError<OutputError, SleepError>
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Engine(error) => error.fmt(formatter),
            Self::Sleep(error) => write!(formatter, "runtime sleep failed: {error}"),
            Self::NotStarted => write!(formatter, "runtime loop has not been started"),
        }
    }
}

impl<OutputError, SleepError> std::error::Error for RuntimeLoopError<OutputError, SleepError>
where
    OutputError: std::fmt::Debug + std::fmt::Display,
    SleepError: std::fmt::Debug + std::fmt::Display,
{
}
