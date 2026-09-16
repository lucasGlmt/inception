use std::convert::Infallible;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use inception_core::{Clock, MonotonicClock, UniverseId};
use inception_driver_dmx::{
    DmxOutput, EnttecDmxUsbProConfig, NullDmxOutput, OpenDmxConfig, RealDmxOutput,
    RealOpenDmxOutput, RecordingDmxOutput, TransportError,
};
use inception_renderer::UniverseFrame;
use inception_runtime::{LoadedProgram, RuntimeConfig, RuntimeHost, RuntimeLoop, StdSleeper};
use lux_cli::ProjectWatcher;
use lux_project::{BuiltProject, ProjectError, discover_project, load_and_build};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("build") => build_command()?,
        Some("check") => {
            let watch = arguments.any(|argument| argument == "--watch");
            check_command(watch)?;
        }
        Some("dev") => {
            let mut output = None;
            while let Some(argument) = arguments.next() {
                if argument == "--output" {
                    output = Some(arguments.next().ok_or("--output requires a driver name")?);
                } else {
                    return Err(format!("unknown dev option `{argument}`").into());
                }
            }
            dev_command(output)?;
        }
        Some("new") => {
            let path = arguments.next().ok_or("lux new requires a project path")?;
            if arguments.next().is_some() {
                return Err("lux new accepts exactly one project path".into());
            }
            new_command(Path::new(&path))?;
        }
        Some(command) => return Err(format!("unknown command `{command}`\n\n{}", usage()).into()),
        None => println!("{}", usage()),
    }
    Ok(())
}

fn usage() -> &'static str {
    "Lux\n\nUSAGE:\n    lux build\n    lux check [--watch]\n    lux dev [--output null|recording|dmx|open-dmx]\n    lux new <path>"
}

fn current_manifest() -> Result<PathBuf, Box<dyn Error>> {
    Ok(discover_project(std::env::current_dir().map_err(
        |source| ProjectError::Io {
            path: PathBuf::from("."),
            source,
        },
    )?)?)
}

fn build_command() -> Result<(), Box<dyn Error>> {
    let built = load_and_build(current_manifest()?).map_err(report_project_error)?;
    println!("✓ project loaded ({})", built.project.manifest.project.name);
    println!("✓ compiled in {} ms", built.timings.compile.as_millis());
    println!("✓ linked in {} ms", built.timings.link.as_millis());
    println!("✓ RuntimeImage ready");
    Ok(())
}

fn check_command(watch: bool) -> Result<(), Box<dyn Error>> {
    let manifest = current_manifest()?;
    let mut built = load_and_build(&manifest).map_err(report_project_error)?;
    println!("✓ {} is valid", built.project.manifest.project.name);
    if !watch {
        return Ok(());
    }
    let running = install_interrupt_handler()?;
    let mut watcher = ProjectWatcher::new(built.project.paths.clone())?;
    println!("Watching project...");
    while running.load(Ordering::Relaxed) {
        if let Some(paths) = watcher.poll(Instant::now())? {
            print_changes(&built.project.paths.root, &paths);
            println!("→ checking");
            match load_and_build(&manifest) {
                Ok(candidate) => {
                    watcher.update_paths(candidate.project.paths.clone());
                    built = candidate;
                    println!("✓ valid ({} ms)", built.timings.total.as_millis());
                }
                Err(error) => {
                    println!("✗ check failed");
                    print_project_error(&error);
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn dev_command(output_override: Option<String>) -> Result<(), Box<dyn Error>> {
    let manifest_path = current_manifest()?;
    let built = load_and_build(&manifest_path).map_err(report_project_error)?;
    let frequency = built.project.manifest.runtime.frequency;
    let output_name = output_override
        .as_deref()
        .unwrap_or(&built.project.manifest.output.driver)
        .to_string();
    let output = open_output(&built, &output_name)?;
    let program = LoadedProgram::new(built.image)
        .map_err(|error| format!("runtime candidate validation failed: {error:?}"))?;
    let mut host = RuntimeHost::from_program(program, output);
    let mut runtime_loop = RuntimeLoop::new(
        RuntimeConfig::new(frequency)?,
        MonotonicClock::new(),
        StdSleeper,
    );
    runtime_loop.start(&mut host)?;

    println!("Lux Dev\n");
    println!("Project: {}", built.project.manifest.project.name);
    println!("Output: {output_name}");
    println!("Frequency: {frequency} Hz\n");
    println!("✓ project loaded");
    println!("✓ compiled");
    println!("✓ linked");
    println!("✓ DMX output connected");
    println!("✓ runtime started @ {frequency} Hz\n");
    println!("Watching project...");
    println!("Commands: b + Enter blackout, s + Enter status, r + Enter reload, q + Enter quit");

    let mut watcher = ProjectWatcher::new(built.project.paths.clone())?;
    let (request_sender, result_receiver) = spawn_build_worker(manifest_path.clone());
    let command_receiver = spawn_stdin_commands();
    let running = install_interrupt_handler()?;
    let mut build_id = 1_u64;
    let mut building = false;
    let initial_output = built.project.manifest.output.clone();

    while running.load(Ordering::Relaxed) {
        runtime_loop.run_next_frame(&mut host)?;

        if let Some(paths) = watcher.poll(Instant::now())? {
            print_changes(&built.project.paths.root, &paths);
            queue_build(&request_sender, &mut building);
        }
        while let Ok(command) = command_receiver.try_recv() {
            match command {
                'b' => {
                    host.blackout()?;
                    println!("✓ blackout sent");
                }
                's' => print_status(build_id, frequency, &runtime_loop, &host),
                'r' => queue_build(&request_sender, &mut building),
                'q' => running.store(false, Ordering::Relaxed),
                _ => {}
            }
        }
        match result_receiver.try_recv() {
            Ok(result) => {
                building = false;
                match result {
                    Ok(candidate) => {
                        let candidate_frequency = candidate.project.manifest.runtime.frequency;
                        if candidate.project.manifest.output != initial_output {
                            println!("✗ output configuration changed; restart required");
                            println!("Keeping build #{build_id} active.");
                        } else if candidate_frequency != frequency {
                            println!("✗ runtime frequency changed; restart required");
                            println!("Keeping build #{build_id} active.");
                        } else {
                            let compile_ms = candidate.timings.compile.as_millis();
                            let link_ms = candidate.timings.link.as_millis();
                            let total_ms = candidate.timings.total.as_millis();
                            let candidate_paths = candidate.project.paths.clone();
                            let loaded = LoadedProgram::new(candidate.image).map_err(|error| {
                                format!("runtime candidate validation failed: {error:?}")
                            })?;
                            let report = host.reload(loaded, runtime_loop.clock().now())?;
                            watcher.update_paths(candidate_paths);
                            build_id += 1;
                            println!("✓ compile ({compile_ms} ms)");
                            println!("✓ link ({link_ms} ms)");
                            println!(
                                "✓ reloaded build #{build_id} ({total_ms} ms, {} fixture(s) preserved)",
                                report.preserved_fixtures
                            );
                        }
                    }
                    Err(error) => {
                        println!("✗ build failed");
                        print_project_error(&error);
                        println!("Keeping build #{build_id} active.");
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Err("build worker disconnected".into()),
        }
    }
    runtime_loop.stop(&mut host)?;
    println!("✓ stopped");
    Ok(())
}

fn queue_build(sender: &SyncSender<()>, building: &mut bool) {
    match sender.try_send(()) {
        Ok(()) => {
            *building = true;
            println!("→ rebuilding");
        }
        Err(TrySendError::Full(())) => {
            // One dirty rebuild is already queued behind the current build.
        }
        Err(TrySendError::Disconnected(())) => eprintln!("build worker disconnected"),
    }
}

fn spawn_build_worker(
    manifest: PathBuf,
) -> (SyncSender<()>, Receiver<Result<BuiltProject, ProjectError>>) {
    let (request_sender, request_receiver) = mpsc::sync_channel(1);
    let (result_sender, result_receiver) = mpsc::channel();
    thread::spawn(move || {
        while request_receiver.recv().is_ok() {
            let result = load_and_build(&manifest);
            if result_sender.send(result).is_err() {
                break;
            }
        }
    });
    (request_sender, result_receiver)
}

fn spawn_stdin_commands() -> Receiver<char> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines().map_while(Result::ok) {
            if let Some(command) = line.trim().chars().next()
                && (sender.send(command).is_err() || command == 'q')
            {
                break;
            }
        }
    });
    receiver
}

fn install_interrupt_handler() -> Result<Arc<AtomicBool>, ctrlc::Error> {
    let running = Arc::new(AtomicBool::new(true));
    let signal_flag = Arc::clone(&running);
    ctrlc::set_handler(move || signal_flag.store(false, Ordering::Relaxed))?;
    Ok(running)
}

fn print_status(
    build_id: u64,
    frequency: u32,
    runtime_loop: &RuntimeLoop<MonotonicClock, StdSleeper>,
    host: &RuntimeHost<CliOutput>,
) {
    let stats = runtime_loop.timing_stats().unwrap_or_default();
    println!("build: #{build_id}");
    println!("frequency: {frequency} Hz");
    println!("frames sent: {}", host.frames_sent());
    println!("late frames: {}", stats.late_frames);
    println!("max lateness: {} ns", stats.max_lateness.0);
    println!("output: connected");
    println!("active transitions: {}", host.active_transition_count());
}

fn print_changes(root: &Path, paths: &[PathBuf]) {
    for path in paths {
        println!(
            "{} changed",
            path.strip_prefix(root).unwrap_or(path).display()
        );
    }
}

fn new_command(root: &Path) -> Result<(), Box<dyn Error>> {
    if root.exists() && fs::read_dir(root)?.next().is_some() {
        return Err(format!("{} already exists and is not empty", root.display()).into());
    }
    fs::create_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("rig"))?;
    fs::create_dir_all(root.join("fixtures"))?;
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("lux-show");
    fs::write(
        root.join("lux.toml"),
        format!(
            r#"[project]
name = "{name}"

[source]
entry = "src/main.lux"

[rig]
patch = "rig/patch.lux"
bindings = "rig/rig.lux"

[fixtures]
directory = "fixtures"

[runtime]
frequency = 40

[output]
driver = "null"
"#
        ),
    )?;
    fs::write(
        root.join("src/main.lux"),
        "rig contract EmptyRig {\n}\n\nscene main {\n}\n",
    )?;
    fs::write(root.join("rig/patch.lux"), "[patch]\nname = \"Empty\"\n")?;
    fs::write(
        root.join("rig/rig.lux"),
        "[rig]\nname = \"EmptyRigBinding\"\ncontract = \"EmptyRig\"\n",
    )?;
    println!("Created Lux project `{name}` at {}", root.display());
    Ok(())
}

fn report_project_error(error: ProjectError) -> ProjectError {
    print_project_error(&error);
    error
}

fn print_project_error(error: &ProjectError) {
    match error {
        ProjectError::Compile(diagnostics) => {
            for diagnostic in diagnostics {
                eprintln!(
                    "error[{:?}] at {}..{}: {}",
                    diagnostic.stage,
                    diagnostic.span.start,
                    diagnostic.span.end,
                    diagnostic.message
                );
                if let Some(help) = &diagnostic.help {
                    eprintln!("  help: {help}");
                }
            }
        }
        other => eprintln!("{other}"),
    }
}

enum CliOutput {
    Null(NullDmxOutput),
    Recording(RecordingDmxOutput),
    Real(Box<dyn DmxOutput<Error = TransportError>>),
}

#[derive(Debug)]
struct CliOutputError(String);

impl fmt::Display for CliOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for CliOutputError {}

impl DmxOutput for CliOutput {
    type Error = CliOutputError;

    fn send(&mut self, universe: UniverseId, frame: &UniverseFrame) -> Result<(), Self::Error> {
        match self {
            Self::Null(output) => output.send(universe, frame).map_err(infallible),
            Self::Recording(output) => output.send(universe, frame).map_err(infallible),
            Self::Real(output) => output
                .send(universe, frame)
                .map_err(|error| CliOutputError(error.to_string())),
        }
    }

    fn close(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Null(output) => output.close().map_err(infallible),
            Self::Recording(output) => output.close().map_err(infallible),
            Self::Real(output) => output
                .close()
                .map_err(|error| CliOutputError(error.to_string())),
        }
    }
}

fn infallible(error: Infallible) -> CliOutputError {
    match error {}
}

fn open_output(built: &BuiltProject, driver: &str) -> Result<CliOutput, Box<dyn Error>> {
    match driver {
        "null" => Ok(CliOutput::Null(NullDmxOutput)),
        "recording" | "dev" => Ok(CliOutput::Recording(RecordingDmxOutput::new())),
        "dmx" | "enttec" => {
            let configured = &built.project.manifest.output;
            let device = configured
                .device
                .as_ref()
                .ok_or("output.device is required for the real DMX driver")?;
            let config = EnttecDmxUsbProConfig::new(
                built.project.paths.root.join(device),
                UniverseId(configured.universe),
            );
            let output: RealDmxOutput = RealDmxOutput::open(config)?;
            Ok(CliOutput::Real(Box::new(output)))
        }
        "open-dmx" => {
            let configured = &built.project.manifest.output;
            let device = configured
                .device
                .as_ref()
                .ok_or("output.device is required for the real DMX driver")?;
            let config = OpenDmxConfig::new(
                built.project.paths.root.join(device),
                UniverseId(configured.universe),
            );
            let output: RealOpenDmxOutput = RealOpenDmxOutput::open(config)?;
            Ok(CliOutput::Real(Box::new(output)))
        }
        unknown => Err(format!("unknown output driver `{unknown}`").into()),
    }
}
