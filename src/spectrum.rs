use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::error::{Error, Result};

/// Frequency band levels from spectral analysis.
#[derive(Debug, Deserialize)]
pub struct SpectrumData {
    pub bands: Bands,
    pub peak: f64,
    pub peak_band: String,
    pub rms: f64,
    #[serde(default)]
    pub peak_db: f64,
    #[serde(default)]
    pub rms_db: f64,
    #[serde(default)]
    pub clipping: bool,
}

#[derive(Debug, Deserialize)]
pub struct Bands {
    pub sub: f64,
    pub low: f64,
    pub low_mid: f64,
    pub mid: f64,
    pub high_mid: f64,
    pub high: f64,
    pub air: f64,
}

const BAND_LABELS: [(&str, &str); 7] = [
    ("sub", "Sub"),
    ("low", "Low"),
    ("low_mid", "Low-Mid"),
    ("mid", "Mid"),
    ("high_mid", "Hi-Mid"),
    ("high", "High"),
    ("air", "Air"),
];

fn spectral_script_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home)
        .join("musictools")
        .join("spectral.py")
}

fn spawn_python(args: &[&str]) -> Result<std::process::Child> {
    let script = spectral_script_path();
    if !script.exists() {
        return Err(Error::Other(format!(
            "spectral.py not found at {}",
            script.display()
        )));
    }

    let cwd = script.parent().unwrap();

    Command::new("uv")
        .arg("run")
        .arg("python3")
        .arg(&script)
        .args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Other(format!("failed to spawn spectral.py: {e}")))
}

fn get_band_level(bands: &Bands, key: &str) -> f64 {
    match key {
        "sub" => bands.sub,
        "low" => bands.low,
        "low_mid" => bands.low_mid,
        "mid" => bands.mid,
        "high_mid" => bands.high_mid,
        "high" => bands.high,
        "air" => bands.air,
        _ => 0.0,
    }
}

fn print_bands(data: &SpectrumData) {
    let bar_width: usize = 40;
    let max_label = 7; // "Low-Mid" / "Hi-Mid"

    for (key, label) in &BAND_LABELS {
        let level = get_band_level(&data.bands, key);
        let filled = ((level * bar_width as f64).round() as usize).min(bar_width);
        let bar: String = "\u{2588}".repeat(filled);

        if level < 0.02 {
            println!("  {:<width$}  -----", label, width = max_label);
        } else {
            println!(
                "  {:<width$}  {:.2}  {}",
                label,
                level,
                bar,
                width = max_label
            );
        }
    }

    print!("  peak: {:.1} dB  rms: {:.1} dB", data.peak_db, data.rms_db);
    if data.clipping {
        print!("  CLIP!");
    }
    println!();
}

/// Check if a JSON line is an error message from the Python script.
fn check_error_line(line: &str) -> Result<()> {
    #[derive(Deserialize)]
    struct ErrorMsg {
        #[allow(dead_code)]
        error: String,
        message: String,
    }

    if let Ok(err) = serde_json::from_str::<ErrorMsg>(line) {
        return Err(Error::Other(err.message));
    }
    Ok(())
}

/// Run spectral analysis via the Python script.
pub fn spectrum(json_mode: bool, continuous: bool, device: Option<&str>) -> Result<()> {
    let mut args: Vec<&str> = Vec::new();
    if continuous {
        args.push("--continuous");
    }

    let dev_string;
    if let Some(d) = device {
        args.push("--device");
        dev_string = d.to_string();
        args.push(&dev_string);
    }

    let mut child = spawn_python(&args)?;

    // Print stderr (status messages) in the background
    let stderr = child.stderr.take();
    let stderr_thread = std::thread::spawn(move || {
        if let Some(err) = stderr {
            let reader = BufReader::new(err);
            for line in reader.lines() {
                if let Ok(line) = line {
                    eprintln!("{}", line);
                }
            }
        }
    });

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);
    let mut printed = false;

    for line in reader.lines() {
        let line = line.map_err(|e| Error::Other(format!("reading spectral output: {e}")))?;

        if line.trim().is_empty() {
            continue;
        }

        if json_mode {
            println!("{}", line);
            printed = true;
            if !continuous {
                break;
            }
            continue;
        }

        // Visual mode: parse JSON and render bars
        let data: SpectrumData = match serde_json::from_str(&line) {
            Ok(d) => d,
            Err(_) => {
                check_error_line(&line)?;
                continue;
            }
        };

        if continuous && printed {
            // Move cursor up to overwrite previous output (7 bands + 1 summary)
            eprint!("\x1b[8A\x1b[0J");
        }

        print_bands(&data);
        printed = true;

        if !continuous {
            break;
        }
    }

    // Wait for process to finish
    let status = child.wait()?;
    let _ = stderr_thread.join();

    if status.code() == Some(2) && !printed {
        return Err(Error::Other(
            "Audio device not found. Run `uv run python3 spectral.py` for details.".into(),
        ));
    } else if !status.success() && !printed {
        return Err(Error::Other(format!(
            "spectral.py exited with code {}",
            status.code().unwrap_or(-1)
        )));
    }

    Ok(())
}

/// Capture a single spectral snapshot (for use by other commands like mix-check).
pub fn capture_snapshot(device: Option<&str>) -> Result<SpectrumData> {
    let mut args: Vec<&str> = Vec::new();

    let dev_string;
    if let Some(d) = device {
        args.push("--device");
        dev_string = d.to_string();
        args.push(&dev_string);
    }

    let child = spawn_python(&args)?;
    let output = child.wait_with_output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Check if it's a device-not-found error
        check_error_line(&stdout)?;
        return Err(Error::Other(format!(
            "spectral.py failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.lines().next().unwrap_or("");

    serde_json::from_str(line).map_err(|e| Error::Other(format!("parsing spectrum data: {e}")))
}
