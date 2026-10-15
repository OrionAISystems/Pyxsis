use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use pyxsis_core::{desktop, recording, store, Region};
use std::path::PathBuf;
mod installer_path;

#[derive(Parser)]
#[command(
    name = "pyxsis-cli",
    version,
    about = "Local Windows capture for people and agents. All command results are JSON."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// Installer-only maintenance of user PATH and an existing startup registration.
    #[command(hide = true)]
    InstallerPath {
        #[arg(long)]
        remove: bool,
    },
    /// Enumerate monitors and visible windows in physical desktop coordinates.
    Targets,
    /// Capture a lossless PNG. Window captures contain visible desktop pixels, including occlusion.
    Capture {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        clipboard: bool,
        #[arg(long)]
        select: bool,
    },
    /// Start, stop, or inspect the per-user recording worker.
    Record {
        #[command(subcommand)]
        command: Record,
    },
    /// Read configuration and capture history.
    Info,
    /// Change the capture root; creates Pictures and Recordings underneath it.
    Configure {
        #[arg(long)]
        capture_root: PathBuf,
    },
}
#[derive(Args)]
struct Target {
    #[arg(long,conflicts_with_all=["window","region"])]
    monitor: Option<usize>,
    #[arg(long,conflicts_with_all=["monitor","region"])]
    window: Option<String>,
    #[arg(long,allow_hyphen_values=true,conflicts_with_all=["monitor","window"],value_parser=parse_region)]
    region: Option<Region>,
}
fn parse_region(value: &str) -> Result<Region, String> {
    let parts: Vec<_> = value.split(',').collect();
    if parts.len() != 4 {
        return Err("Use x,y,width,height in physical pixels".into());
    }
    Ok(Region {
        x: parts[0].parse().map_err(|_| "Invalid x")?,
        y: parts[1].parse().map_err(|_| "Invalid y")?,
        width: parts[2].parse().map_err(|_| "Invalid width")?,
        height: parts[3].parse().map_err(|_| "Invalid height")?,
    })
}
#[derive(Subcommand)]
enum Record {
    Start {
        #[command(flatten)]
        target: Target,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value_t = 30)]
        fps: u32,
        #[arg(long)]
        system_audio: bool,
        #[arg(long, default_value_t = 300)]
        duration: u32,
        #[arg(long)]
        select: bool,
    },
    Stop,
    Status,
    #[command(hide = true)]
    Run {
        #[arg(long)]
        session: String,
    },
}
fn run() -> Result<serde_json::Value> {
    desktop::init_dpi();
    let cli = Cli::parse();
    Ok(match cli.command {
        Commands::InstallerPath { remove } => {
            installer_path::update(remove)?;
            let gui = std::env::current_exe()?
                .parent()
                .expect("CLI directory")
                .join("pyxsis.exe");
            pyxsis_core::startup::maintain_install(&gui, remove)?;
            serde_json::json!({"ok":true})
        }
        Commands::Targets => {
            serde_json::json!({"monitors":desktop::monitors(),"windows":desktop::windows(),"coordinate_space":"physical_desktop_pixels","window_capture":"visible_pixels"})
        }
        Commands::Capture {
            target,
            output,
            clipboard,
            select,
        } => {
            if select
                && (target.monitor.is_some() || target.window.is_some() || target.region.is_some())
            {
                bail!("--select cannot be combined with target arguments");
            }
            if select {
                match desktop::select_screenshot(output, clipboard)? {
                    Some(artifact) => serde_json::to_value(artifact)?,
                    None => serde_json::json!({"cancelled":true}),
                }
            } else {
                let region =
                    desktop::resolve(target.monitor, target.window.as_deref(), target.region)?;
                serde_json::to_value(desktop::screenshot(region, output, clipboard)?)?
            }
        }
        Commands::Record { command } => match command {
            Record::Start {
                target,
                output,
                fps,
                system_audio,
                duration,
                select,
            } => {
                if select
                    && (target.monitor.is_some()
                        || target.window.is_some()
                        || target.region.is_some())
                {
                    bail!("--select cannot be combined with target arguments");
                }
                let region = if select {
                    match desktop::select_region()? {
                        Some(r) => r,
                        None => return Ok(serde_json::json!({"cancelled":true})),
                    }
                } else {
                    desktop::resolve(target.monitor, target.window.as_deref(), target.region)?
                };
                serde_json::to_value(recording::start(
                    &std::env::current_exe()?,
                    region,
                    output,
                    fps,
                    system_audio,
                    duration,
                )?)?
            }
            Record::Stop => serde_json::to_value(recording::stop()?)?,
            Record::Status => match recording::status()? {
                Some(session) => serde_json::to_value(session)?,
                None => serde_json::json!({"state":"idle","session":null}),
            },
            Record::Run { session } => {
                recording::run_worker(&session)?;
                serde_json::json!({"ok":true})
            }
        },
        Commands::Info => {
            serde_json::json!({"preferences":store::preferences()?,"history":store::history()?,"recording":recording::status()?})
        }
        Commands::Configure { capture_root } => {
            let mut prefs = store::preferences()?;
            prefs.capture_root = capture_root;
            store::save_preferences(&prefs)?;
            serde_json::to_value(prefs)?
        }
    })
}
fn main() {
    match run() {
        Ok(value) => println!("{}", serde_json::to_string(&value).unwrap()),
        Err(error) => {
            eprintln!("{}", serde_json::json!({"error":format!("{error:#}")}));
            std::process::exit(1);
        }
    }
}
