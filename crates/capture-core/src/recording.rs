use crate::encoder::Encoder;
use crate::{
    desktop::{self, Surface},
    store::{self, Artifact},
    Region,
};
use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub state: String,
    pub path: PathBuf,
    pub region: Region,
    pub fps: u32,
    pub system_audio: bool,
    pub started_at: u64,
    pub duration_limit: u32,
    pub error: Option<String>,
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub metrics: RecordingMetrics,
    #[serde(default)]
    pub worker_pid: Option<u32>,
}
/// Submission metrics describe acquisition and encoder enqueueing, not encoded
/// output frame counts. Older session files remain readable through serde defaults.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingMetrics {
    pub frames_submitted: u64,
    pub missed_frame_intervals: u64,
    pub capture_ms: u64,
    pub finalization_ms: u64,
    pub acquisition_ms: u64,
    pub cpu_ms: Option<u64>,
    pub peak_private_bytes: usize,
}
fn lock(name: &str) -> Result<File> {
    fs::create_dir_all(store::data_dir())?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(store::data_dir().join(name))?;
    file.try_lock_exclusive()
        .context("A recording operation is already running")?;
    Ok(file)
}
fn session_path() -> PathBuf {
    store::data_dir().join("recording.json")
}
fn stop_path(id: &str) -> PathBuf {
    store::data_dir().join(format!("stop-{id}"))
}
pub fn status() -> Result<Option<Session>> {
    if !session_path().exists() {
        return Ok(None);
    }
    let mut session: Session = serde_json::from_slice(&fs::read(session_path())?)?;
    if ["starting", "recording", "finalizing"].contains(&session.state.as_str())
        && lock("recording.lock").is_ok()
    {
        // A crashed worker cannot leave the application stuck in Recording forever.
        if store::now().saturating_sub(session.started_at) > 15_000 {
            session.state = "interrupted".into();
            session.error=Some("Recording worker exited unexpectedly; partial file may remain next to the requested output".into());
        }
    }
    Ok(Some(session))
}
pub fn start(
    worker: &Path,
    region: Region,
    output: Option<PathBuf>,
    fps: u32,
    audio: bool,
    duration: u32,
) -> Result<Session> {
    let _start_lock = lock("start.lock")?;
    let check = lock("recording.lock").context("A recording is already active")?;
    if ![15, 30, 60].contains(&fps) || !(1..=1800).contains(&duration) {
        bail!("Choose 15/30/60 FPS and a duration from 1 to 1800 seconds");
    }
    let mut region = region.validate(desktop::bounds())?;
    region.width &= !1;
    region.height &= !1;
    if u64::from(region.width) * u64::from(region.height) > 8_294_400 {
        bail!("Recording is limited to 4K-equivalent area; select a smaller region");
    }
    let path = store::output_path("recording", output)?;
    if path.to_string_lossy().starts_with("\\\\") {
        bail!("Record to a local drive, then copy the completed MP4 to a network share");
    }
    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        state: "starting".into(),
        path,
        region,
        fps,
        system_audio: audio,
        started_at: store::now(),
        duration_limit: duration,
        error: None,
        artifact: None,
        metrics: RecordingMetrics::default(),
        worker_pid: None,
    };
    store::atomic_json(&session_path(), &session)?;
    drop(check);
    let child = match WorkerProcess::spawn(worker, &session.id) {
        Ok(child) => child,
        Err(error) => {
            let mut failed = session.clone();
            failed.state = "failed".into();
            failed.error = Some(error.to_string());
            store::atomic_json(&session_path(), &failed)?;
            return Err(error);
        }
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let current: Session = serde_json::from_slice(&fs::read(session_path())?)?;
        if current.state == "recording" || current.state == "completed" {
            return Ok(current);
        }
        if current.state == "failed" {
            bail!("{}", current.error.unwrap_or("Recording failed".into()));
        }
        if let Some(exit) = child.exit_code()? {
            bail!("Recording worker exited before it became ready ({exit})");
        }
        thread::sleep(Duration::from_millis(100));
    }
    // Request cooperative cancellation; never kill an encoder during finalization.
    fs::write(stop_path(&session.id), b"stop")?;
    bail!("Recording startup timed out; cancellation requested")
}
/// No handle inheritance: an agent waiting for CLI JSON must not wait for the
/// worker to close inherited stdout pipes. Standard Command redirection alone
/// does not exclude all inherited Windows handles in a nested process tree.
struct WorkerProcess(windows_sys::Win32::Foundation::HANDLE);
impl WorkerProcess {
    fn spawn(worker: &Path, id: &str) -> Result<Self> {
        use windows_sys::Win32::System::Threading::*;
        let application = desktop::wide(&worker.to_string_lossy());
        let mut command = desktop::wide(&format!(
            "\"{}\" record run --session {id}",
            worker.display()
        ));
        unsafe {
            let mut startup: STARTUPINFOW = std::mem::zeroed();
            startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            let mut process: PROCESS_INFORMATION = std::mem::zeroed();
            if CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                std::ptr::null(),
                &startup,
                &mut process,
            ) == 0
            {
                bail!(
                    "Cannot launch recording worker: {}",
                    std::io::Error::last_os_error()
                );
            }
            windows_sys::Win32::Foundation::CloseHandle(process.hThread);
            Ok(Self(process.hProcess))
        }
    }
    fn exit_code(&self) -> Result<Option<u32>> {
        unsafe {
            let mut code = 0;
            if windows_sys::Win32::System::Threading::GetExitCodeProcess(self.0, &mut code) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(if code == 259 { None } else { Some(code) })
        }
    }
}
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
pub fn stop() -> Result<Session> {
    let session = status()?.context("No recording session exists")?;
    if !["starting", "recording", "finalizing"].contains(&session.state.as_str()) {
        bail!("There is no active recording");
    }
    fs::write(stop_path(&session.id), b"stop")?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        let current = status()?.context("Recording state disappeared")?;
        if !["starting", "recording", "finalizing"].contains(&current.state.as_str()) {
            if current.state == "completed" {
                return Ok(current);
            }
            bail!(
                "Recording did not complete: {}",
                current.error.as_deref().unwrap_or(&current.state)
            );
        }
        thread::sleep(Duration::from_millis(100));
    }
    bail!("Stop requested; Windows is still finalizing the MP4. Check record status before exiting")
}
struct Loopback {
    _stream: cpal::Stream,
    receiver: Receiver<Vec<i16>>,
    errors: Receiver<String>,
    pending: VecDeque<i16>,
    sample_rate: u32,
    channels: u32,
}
impl Loopback {
    fn new() -> Result<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .context("No default system audio output device")?;
        let supported = device.default_output_config()?;
        let config = supported.config();
        let (tx, receiver) = mpsc::sync_channel(100);
        let (error_tx, errors) = mpsc::channel();
        let on_error = move |error| {
            let _ = error_tx.send(format!("{error}"));
        };
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config,
                move |data: &[f32], _| {
                    let samples = data
                        .iter()
                        .map(|v| (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                        .collect();
                    let _ = tx.try_send(samples);
                },
                on_error,
                None,
            )?,
            cpal::SampleFormat::I16 => device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let _ = tx.try_send(data.to_vec());
                },
                on_error,
                None,
            )?,
            cpal::SampleFormat::I32 => device.build_input_stream(
                &config,
                move |data: &[i32], _| {
                    let _ = tx.try_send(data.iter().map(|v| (v >> 16) as i16).collect());
                },
                on_error,
                None,
            )?,
            format => bail!("Unsupported system audio format: {format:?}"),
        };
        Ok(Self {
            _stream: stream,
            receiver,
            errors,
            pending: VecDeque::new(),
            sample_rate: config.sample_rate.0,
            channels: config.channels as u32,
        })
    }
    fn take(&mut self, frames: usize) -> Result<Vec<u8>> {
        if let Ok(error) = self.errors.try_recv() {
            bail!("System audio stream failed: {error}");
        }
        for chunk in self.receiver.try_iter() {
            self.pending.extend(chunk);
        }
        let length = frames * self.channels as usize;
        // Keep latency bounded if the capture thread stalls; silence is explicit for idle endpoints.
        while self.pending.len() > self.sample_rate as usize * self.channels as usize / 2 {
            self.pending.pop_front();
        }
        let mut bytes = Vec::with_capacity(length * 2);
        for _ in 0..length {
            bytes.extend_from_slice(&self.pending.pop_front().unwrap_or(0).to_le_bytes());
        }
        Ok(bytes)
    }
}
pub fn run_worker(id: &str) -> Result<()> {
    let _lock = lock("recording.lock")?;
    let mut session: Session = serde_json::from_slice(&fs::read(session_path())?)?;
    if session.id != id || session.state != "starting" {
        bail!("Stale or invalid recording session");
    }
    session.worker_pid = Some(std::process::id());
    let result = record(&mut session);
    if let Err(error) = &result {
        session.state = "failed".into();
        session.error = Some(format!("{error:#}"));
        store::log_event("recording.failed", &format!("{} {error:#}", session.id));
        store::atomic_json(&session_path(), &session)?;
    }
    let _ = fs::remove_file(stop_path(id));
    result
}
fn record(session: &mut Session) -> Result<()> {
    unsafe {
        windows::Win32::System::WinRT::RoInitialize(
            windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
        )?;
    }
    let mut surface = Surface::new(session.region)?;
    let mut loopback = if session.system_audio {
        Some(Loopback::new()?)
    } else {
        None
    };
    let audio = loopback
        .as_ref()
        .map(|audio| (audio.sample_rate, audio.channels));
    let partial = session
        .path
        .with_file_name(format!(".pyxsis-{}.partial.mp4", session.id));
    let reservation = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&session.path)
        .context("Output already exists")?;
    drop(reservation);
    let result = (|| -> Result<Artifact> {
        let mut encoder = Encoder::new(&partial, session.region.width, session.region.height, session.fps, audio)
            .context("Windows could not initialize H.264/AAC encoding; try a smaller region or disable system audio")?;
        let hud = desktop::RecordingHud::spawn(session.region, stop_path(&session.id))?;
        if let Some(audio) = &loopback {
            audio._stream.play()?;
        }
        session.state = "recording".into();
        session.started_at = store::now();
        store::atomic_json(&session_path(), session)?;
        store::log_event("recording.started", &session.id);
        let cpu_start = process_cpu_ms().ok();
        let start = Instant::now();
        let interval = Duration::from_secs_f64(1.0 / session.fps as f64);
        let mut next_frame = start;
        let mut audio_sent = 0usize;
        let mut acquisition_time = Duration::ZERO;
        let capture_result = (|| -> Result<()> {
            loop {
                session.metrics.peak_private_bytes = session
                    .metrics
                    .peak_private_bytes
                    .max(check_memory_budget()?);
                let acquired = Instant::now();
                let timestamp = (start.elapsed().as_nanos() / 100) as i64;
                let frame = surface.frame()?;
                acquisition_time += acquired.elapsed();
                encoder.send_frame_buffer(frame, timestamp)?;
                session.metrics.frames_submitted += 1;
                if let Some(audio) = &mut loopback {
                    let clock_due =
                        (start.elapsed().as_secs_f64() * audio.sample_rate as f64) as usize;
                    if clock_due > audio_sent {
                        encoder
                            .send_audio_buffer(&audio.take(clock_due - audio_sent)?, timestamp)?;
                        audio_sent = clock_due;
                    }
                }
                if stop_path(&session.id).exists()
                    || start.elapsed().as_secs() >= session.duration_limit as u64
                {
                    break;
                }
                next_frame += interval;
                let now = Instant::now();
                if next_frame <= now {
                    let missed =
                        now.duration_since(next_frame).as_nanos() / interval.as_nanos() + 1;
                    session.metrics.missed_frame_intervals += missed as u64;
                    next_frame = now + interval;
                }
                thread::sleep(next_frame.saturating_duration_since(now));
            }
            Ok(())
        })();
        session.state = "finalizing".into();
        let capture_duration = start.elapsed();
        session.metrics.capture_ms = capture_duration.as_millis() as u64;
        session.metrics.acquisition_ms = acquisition_time.as_millis() as u64;
        if let Some(hud) = &hud {
            hud.finalizing();
        }
        store::atomic_json(&session_path(), session)?;
        // Finalize even when capture or audio fails so existing frames are recoverable.
        let finalization_start = Instant::now();
        let finalized = encoder.finish();
        session.metrics.finalization_ms = finalization_start.elapsed().as_millis() as u64;
        session.metrics.cpu_ms = cpu_start
            .zip(process_cpu_ms().ok())
            .map(|(start, end)| end.saturating_sub(start));
        store::log_event(
            "recording.metrics",
            &serde_json::to_string(&session.metrics)?,
        );
        finalized?;
        capture_result?;
        desktop::replace_file(&partial, &session.path)?;
        Ok(Artifact {
            id: session.id.clone(),
            kind: "recording".into(),
            path: session.path.clone(),
            width: session.region.width,
            height: session.region.height,
            created_at: session.started_at,
            bytes: fs::metadata(&session.path)?.len(),
            duration_ms: Some(capture_duration.as_millis() as u64),
            system_audio: session.system_audio,
            warnings: vec![],
            region: Some(session.region),
            coordinate_space: store::physical_coordinates(),
        })
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&session.path);
        return Err(error);
    }
    let artifact = result?;
    store::remember(&artifact)?;
    session.artifact = Some(artifact);
    session.state = "completed".into();
    store::atomic_json(&session_path(), session)?;
    store::log_event("recording.completed", &session.id);
    Ok(())
}
fn process_cpu_ms() -> Result<u64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    unsafe {
        let mut created: FILETIME = std::mem::zeroed();
        let mut exited: FILETIME = std::mem::zeroed();
        let mut kernel: FILETIME = std::mem::zeroed();
        let mut user: FILETIME = std::mem::zeroed();
        if GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        ) == 0
        {
            bail!(
                "Cannot inspect recording CPU usage: {}",
                std::io::Error::last_os_error()
            );
        }
        let ticks = |value: FILETIME| {
            (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
        };
        Ok((ticks(kernel) + ticks(user)) / 10_000)
    }
}
fn check_memory_budget() -> Result<usize> {
    use windows_sys::Win32::System::{ProcessStatus::*, Threading::GetCurrentProcess};
    unsafe {
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
            counters.cb,
        ) == 0
        {
            bail!(
                "Cannot inspect recording memory usage: {}",
                std::io::Error::last_os_error()
            );
        }
        if counters.PrivateUsage > 512 * 1024 * 1024 {
            bail!("Recording exceeded its 512 MiB encoder memory budget; choose a smaller region or lower frame rate");
        }
        Ok(counters.PrivateUsage)
    }
}
