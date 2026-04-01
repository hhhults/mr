//! Audio commands: slice, analyze, separate, morph-audio, export.
//!
//! Wraps FluCoMa CLI tools via the corpus crate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use corpus::flucoma::Config;
use serde_json::{json, Value};

use crate::error::{Error, Result};
use crate::json::{self, MrData, MrGrain};
use crate::track;

/// Resolve an audio file path with tilde expansion and existence check.
fn resolve_audio(path: &str) -> Result<PathBuf> {
    let expanded = track::shellexpand(path);
    let p = PathBuf::from(&expanded);
    if !p.exists() {
        return Err(Error::Other(format!("audio file not found: {}", p.display())));
    }
    Ok(p)
}

/// Default output directory: ./chops/<stem>/ or ./separated/<stem>/
fn default_out_dir(input: &Path, prefix: &str) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    PathBuf::from(prefix).join(stem)
}

// ── mr slice ────────────────────────────────────────────────────────

pub fn slice(
    audio_file: &str,
    method: &str,
    threshold: Option<f64>,
    out_dir: Option<&str>,
    min_length_ms: Option<f64>,
) -> Result<()> {
    let input = resolve_audio(audio_file)?;
    let config = Config::new();
    let info = corpus::audio::wav_info(&input)?;

    // Convert min length from ms to frames
    let min_frames = min_length_ms
        .map(|ms| (ms / 1000.0 * info.sample_rate as f64) as usize)
        .unwrap_or(0);

    // Run slicer
    let slice_points = match method {
        "onset" => {
            let mut opts = corpus::flucoma::slice::OnsetOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::onset_slice(&input, &opts, &config)?
        }
        "novelty" => {
            let mut opts = corpus::flucoma::slice::NoveltyOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::novelty_slice(&input, &opts, &config)?
        }
        "amp" => {
            let mut opts = corpus::flucoma::slice::AmpOpts::default();
            if let Some(t) = threshold {
                opts.on_threshold = t;
            }
            corpus::flucoma::slice::amp_slice(&input, &opts, &config)?
        }
        "transient" => {
            let mut opts = corpus::flucoma::slice::TransientOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::transient_slice(&input, &opts, &config)?
        }
        _ => {
            return Err(Error::Other(format!(
                "unknown slice method: \"{method}\"\navailable: onset, novelty, amp, transient"
            )));
        }
    };

    // Split WAV into grains
    let out = match out_dir {
        Some(d) => PathBuf::from(track::shellexpand(d)),
        None => default_out_dir(&input, "chops"),
    };
    let results = corpus::audio::split_wav(&input, &slice_points, &out, min_frames)?;

    let source_name = input
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Build MrData::Grains for pipe output
    let grains: Vec<MrGrain> = results
        .iter()
        .map(|r| MrGrain {
            path: r.path.to_string_lossy().to_string(),
            index: r.index,
            start: r.start_sample as f64 / info.sample_rate as f64,
            duration: r.duration_secs,
            source: source_name.clone(),
        })
        .collect();

    eprintln!(
        "sliced {} grains → {}",
        grains.len(),
        out.display()
    );

    json::write_stdout(&MrData::Grains {
        source: source_name,
        sample_rate: info.sample_rate,
        grains,
    })?;

    Ok(())
}

// ── mr analyze ──────────────────────────────────────────────────────

pub fn analyze(
    path: Option<&str>,
    name: &str,
    features_filter: Option<&str>,
) -> Result<()> {
    let config = Config::new();

    // Determine which features to extract
    let extract_mfcc;
    let extract_spectral;
    let extract_pitch;
    let extract_loudness;
    if let Some(filter) = features_filter {
        let parts: Vec<&str> = filter.split(',').collect();
        extract_mfcc = parts.contains(&"mfcc");
        extract_spectral = parts.contains(&"spectral");
        extract_pitch = parts.contains(&"pitch");
        extract_loudness = parts.contains(&"loudness");
    } else {
        extract_mfcc = true;
        extract_spectral = true;
        extract_pitch = true;
        extract_loudness = true;
    }

    // Collect audio files: from path argument, or from stdin (grains pipe)
    let files: Vec<(PathBuf, String, f64, f64)> = if let Some(p) = path {
        let expanded = PathBuf::from(track::shellexpand(p));
        if expanded.is_dir() {
            let mut wavs: Vec<_> = std::fs::read_dir(&expanded)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "wav")
                        .unwrap_or(false)
                })
                .map(|e| {
                    let p = e.path();
                    let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                    (p, name, 0.0, 0.0)
                })
                .collect();
            wavs.sort_by(|a, b| a.0.cmp(&b.0));
            wavs
        } else {
            let name = expanded
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            vec![(expanded, name, 0.0, 0.0)]
        }
    } else if !json::atty_stdin() {
        // No path argument — try reading grains from stdin pipe
        let data = json::read_stdin()?;
        let (_source, _sr, grains) = data.into_grains("analyze")?;
        grains
            .iter()
            .map(|g| {
                (
                    PathBuf::from(&g.path),
                    g.source.clone(),
                    g.start,
                    g.duration,
                )
            })
            .collect()
    } else {
        return Err(Error::Other(
            "analyze requires a path argument or piped grains from mr slice".to_string(),
        ));
    };

    if files.is_empty() {
        return Err(Error::Other("no WAV files found".to_string()));
    }

    eprintln!("analyzing {} files...", files.len());

    let mut corpus = corpus::Corpus::new();

    for (i, (file_path, source, start, duration)) in files.iter().enumerate() {
        if !file_path.exists() {
            eprintln!("  skip (missing): {}", file_path.display());
            continue;
        }

        eprint!("\r  {}/{} {}", i + 1, files.len(), source);

        let mut feature_values: Vec<f64> = Vec::new();

        // MFCC: take mean of each coefficient (skip coeff 0 = energy)
        if extract_mfcc {
            match corpus::flucoma::analyze::mfcc(file_path, 13, &config) {
                Ok(frames) if !frames.is_empty() => {
                    let n = frames.len() as f64;
                    let dims = frames[0].len();
                    for d in 1..dims {
                        // skip coeff 0
                        let mean: f64 = frames.iter().map(|f| f[d]).sum::<f64>() / n;
                        feature_values.push(mean);
                    }
                }
                Err(e) => {
                    if i == 0 {
                        eprintln!("\n  warning: mfcc failed: {e}");
                    }
                    for _ in 0..12 {
                        feature_values.push(0.0);
                    }
                }
                _ => {
                    for _ in 0..12 {
                        feature_values.push(0.0);
                    }
                }
            }
        }

        // Spectral shape: mean of centroid, spread, flatness
        if extract_spectral {
            match corpus::flucoma::analyze::spectral_shape(file_path, &config) {
                Ok(frames) if !frames.is_empty() => {
                    let n = frames.len() as f64;
                    let centroid: f64 = frames.iter().map(|f| f.centroid).sum::<f64>() / n;
                    let spread: f64 = frames.iter().map(|f| f.spread).sum::<f64>() / n;
                    let flatness: f64 = frames.iter().map(|f| f.flatness).sum::<f64>() / n;
                    feature_values.push(centroid);
                    feature_values.push(spread);
                    feature_values.push(flatness);
                }
                _ => {
                    feature_values.extend_from_slice(&[0.0, 0.0, 0.0]);
                }
            }
        }

        // Pitch: median of confident frames
        if extract_pitch {
            match corpus::flucoma::analyze::pitch(file_path, &config) {
                Ok(frames) if !frames.is_empty() => {
                    let confident: Vec<f64> = frames
                        .iter()
                        .filter(|f| f.confidence > 0.5)
                        .map(|f| f.hz)
                        .collect();
                    let pitch = if confident.is_empty() {
                        0.0
                    } else {
                        let mut sorted = confident.clone();
                        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        sorted[sorted.len() / 2]
                    };
                    feature_values.push(pitch);
                }
                _ => {
                    feature_values.push(0.0);
                }
            }
        }

        // Loudness: mean LUFS
        if extract_loudness {
            match corpus::flucoma::analyze::loudness(file_path, &config) {
                Ok(frames) if !frames.is_empty() => {
                    let n = frames.len() as f64;
                    let mean: f64 = frames.iter().map(|f| f.loudness).sum::<f64>() / n;
                    feature_values.push(mean);
                }
                _ => {
                    feature_values.push(0.0);
                }
            }
        }

        let dur = if *duration > 0.0 {
            *duration
        } else {
            corpus::audio::wav_info(file_path)
                .map(|i| i.duration_secs)
                .unwrap_or(0.0)
        };

        corpus.ingest(
            source,
            *start,
            dur,
            corpus::Features::new(feature_values),
        );
    }

    eprintln!();

    corpus.normalize();
    let save_path = corpus::persist::save(&corpus, name)?;
    eprintln!(
        "analyzed {} grains → {}",
        corpus.len(),
        save_path.display()
    );

    Ok(())
}

// ── mr separate ─────────────────────────────────────────────────────

pub fn separate(
    audio_file: &str,
    method: &str,
    components: usize,
    out_dir: Option<&str>,
    load_prefix: Option<&str>,
) -> Result<()> {
    let input = resolve_audio(audio_file)?;
    let config = Config::new();

    let out = match out_dir {
        Some(d) => PathBuf::from(track::shellexpand(d)),
        None => default_out_dir(&input, "separated"),
    };

    let component_files: Vec<(String, PathBuf)> = match method {
        "hpss" => {
            let result = corpus::flucoma::decompose::hpss(&input, &out, &config)?;
            vec![
                ("harmonic".into(), result.harmonic),
                ("percussive".into(), result.percussive),
            ]
        }
        "sines" => {
            let result = corpus::flucoma::decompose::sines(&input, &out, &config)?;
            vec![
                ("sines".into(), result.sines),
                ("residual".into(), result.residual),
            ]
        }
        "transients" => {
            let result = corpus::flucoma::decompose::transients(&input, &out, &config)?;
            vec![
                ("transients".into(), result.transients),
                ("residual".into(), result.residual),
            ]
        }
        "nmf" => {
            let result =
                corpus::flucoma::decompose::nmf(&input, components, &out, &config)?;
            result
                .components
                .into_iter()
                .enumerate()
                .map(|(i, p)| (format!("component_{i}"), p))
                .collect()
        }
        _ => {
            return Err(Error::Other(format!(
                "unknown separate method: \"{method}\"\navailable: hpss, sines, transients, nmf"
            )));
        }
    };

    for (name, path) in &component_files {
        eprintln!("  {name}: {}", path.display());
    }

    // Load into Ableton if requested
    if let Some(prefix) = load_prefix {
        for (name, path) in &component_files {
            let track_name = format!("{prefix}_{name}");
            let path_str = path.to_string_lossy().to_string();
            match track::stage_sample(&path_str) {
                Ok(filename) => {
                    eprintln!("  loading {track_name} ← {filename}");
                    // Use the existing track creation infrastructure
                    track::create(&track_name, Some(&path_str), None, false)?;
                }
                Err(e) => {
                    eprintln!("  warning: could not stage {name}: {e}");
                }
            }
        }
    }

    Ok(())
}

// ── mr morph-audio ──────────────────────────────────────────────────

pub fn morph_audio(
    file_a: &str,
    file_b: &str,
    amount: f64,
    steps: Option<usize>,
    out_path: Option<&str>,
) -> Result<()> {
    let a = resolve_audio(file_a)?;
    let b = resolve_audio(file_b)?;
    let config = Config::new();

    let amounts: Vec<f64> = if let Some(n) = steps {
        if n < 2 {
            vec![amount]
        } else {
            (0..n).map(|i| i as f64 / (n - 1) as f64).collect()
        }
    } else {
        vec![amount]
    };

    let stem_a = a
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("a");
    let stem_b = b
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("b");

    for (i, &amt) in amounts.iter().enumerate() {
        let output = if let Some(p) = out_path {
            let expanded = PathBuf::from(track::shellexpand(p));
            if amounts.len() > 1 {
                // Multiple steps → treat as directory
                std::fs::create_dir_all(&expanded)?;
                expanded.join(format!("{stem_a}_{stem_b}_{i:02}_{:.0}pct.wav", amt * 100.0))
            } else if expanded.extension().is_some() {
                // Single step with file extension → use as-is
                expanded
            } else {
                // Single step, no extension → treat as directory
                std::fs::create_dir_all(&expanded)?;
                expanded.join(format!("{stem_a}_{stem_b}_{:.0}pct.wav", amt * 100.0))
            }
        } else {
            PathBuf::from(format!(
                "{stem_a}_{stem_b}_{:.0}pct.wav",
                amt * 100.0
            ))
        };

        let opts = corpus::flucoma::audiotransport::AudioTransportOpts {
            interpolation: amt,
        };
        corpus::flucoma::audiotransport::audiotransport(&a, &b, &output, &opts, &config)?;
        eprintln!(
            "  {:.0}% → {}",
            amt * 100.0,
            output.display()
        );
    }

    Ok(())
}

// ── mr export ───────────────────────────────────────────────────────

/// Export a grain folder as an SP-Tools compatible corpus JSON.
///
/// Re-analyzes each WAV file with FluCoMa to produce the 8 descriptors
/// (loudness, loudness_deriv, centroid, centroid_deriv, flatness,
/// flatness_deriv, pitch, pitch_confidence), 40 melbands, and builds
/// the coll, normalized, and robustscaled data that SP-Tools expects.
pub fn export_sp_tools(
    grain_dir: &str,
    output: &str,
) -> Result<()> {
    let dir = PathBuf::from(track::shellexpand(grain_dir));
    if !dir.is_dir() {
        return Err(Error::Other(format!("not a directory: {}", dir.display())));
    }

    let config = Config::new();

    // Collect WAV files
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "wav")
                .unwrap_or(false)
        })
        .map(|e| e.path())
        .collect();
    wavs.sort();

    if wavs.is_empty() {
        return Err(Error::Other("no WAV files found in directory".to_string()));
    }

    eprintln!("exporting {} files for SP-Tools...", wavs.len());

    // Per-grain analysis results
    let mut descriptors: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut melbands_data: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut coll_data: BTreeMap<String, Vec<f64>> = BTreeMap::new();

    // Stats accumulators for meta
    let mut total_duration_ms = 0.0f64;
    let mut min_duration_ms = f64::MAX;
    let mut max_duration_ms = 0.0f64;
    let mut sum_loudness = 0.0f64;
    let mut sum_centroid = 0.0f64;
    let mut sum_flatness = 0.0f64;
    let mut sum_pitch = 0.0f64;
    let mut sum_pitch_conf = 0.0f64;
    let mut sum_time_centroid = 0.0f64;
    let mut sample_rate = 44100u32;

    for (i, wav_path) in wavs.iter().enumerate() {
        let key = (i + 1).to_string();
        eprint!("\r  {}/{} {}", i + 1, wavs.len(),
            wav_path.file_name().unwrap_or_default().to_string_lossy());

        let info = match corpus::audio::wav_info(wav_path) {
            Ok(i) => i,
            Err(_) => continue,
        };
        sample_rate = info.sample_rate;
        let dur_ms = info.duration_secs * 1000.0;
        total_duration_ms += dur_ms;
        min_duration_ms = min_duration_ms.min(dur_ms);
        max_duration_ms = max_duration_ms.max(dur_ms);

        // 8 descriptors: loudness, loud_deriv, centroid, cent_deriv,
        //                flatness, flat_deriv, pitch, pitch_conf
        let (mean_loud, loud_deriv) =
            analyze_with_deriv(wav_path, &config, AnalysisKind::Loudness);
        let (mean_cent, cent_deriv) =
            analyze_with_deriv(wav_path, &config, AnalysisKind::Centroid);
        let (mean_flat, flat_deriv) =
            analyze_with_deriv(wav_path, &config, AnalysisKind::Flatness);
        let (mean_pitch, pitch_conf) = analyze_pitch_summary(wav_path, &config);

        let desc_vec = vec![
            mean_loud, loud_deriv, mean_cent, cent_deriv,
            mean_flat, flat_deriv, mean_pitch, pitch_conf,
        ];

        sum_loudness += mean_loud;
        sum_centroid += mean_cent;
        sum_flatness += mean_flat;
        sum_pitch += mean_pitch;
        sum_pitch_conf += pitch_conf;

        // Melbands (40 bands)
        let mel = match corpus::flucoma::analyze::melbands(wav_path, 40, &config) {
            Ok(frames) if !frames.is_empty() => {
                let n = frames.len() as f64;
                let dims = frames[0].len().min(40);
                (0..dims)
                    .map(|d| frames.iter().map(|f| f.get(d).copied().unwrap_or(0.0)).sum::<f64>() / n)
                    .collect::<Vec<f64>>()
            }
            _ => vec![0.0; 40],
        };

        // Time centroid: weighted average of frame times by loudness
        let time_centroid = match corpus::flucoma::analyze::loudness(wav_path, &config) {
            Ok(frames) if frames.len() > 1 => {
                let total_loud: f64 = frames.iter().map(|f| f.loudness.abs()).sum();
                if total_loud > 0.0 {
                    let weighted: f64 = frames.iter().enumerate()
                        .map(|(i, f)| {
                            let t_ms = i as f64 / frames.len() as f64 * dur_ms;
                            t_ms * f.loudness.abs()
                        })
                        .sum::<f64>();
                    weighted / total_loud
                } else {
                    dur_ms / 2.0
                }
            }
            _ => dur_ms / 2.0,
        };
        sum_time_centroid += time_centroid;

        // Coll entry: 40 melbands + loudness + count + duration_samples + time_centroid + centroid
        let dur_samples = info.num_frames as f64;
        let mut coll_entry = mel.clone();
        coll_entry.push(mean_loud);
        coll_entry.push(1.0); // sample count
        coll_entry.push(dur_samples);
        coll_entry.push(time_centroid);
        coll_entry.push(mean_cent);

        descriptors.insert(key.clone(), desc_vec);
        melbands_data.insert(key.clone(), mel);
        coll_data.insert(key, coll_entry);
    }

    let n = wavs.len() as f64;
    eprintln!("\n  building SP-Tools JSON...");

    // Normalize descriptors (min-max → 0..1)
    let (desc_norm, desc_norm_fit) = normalize_dataset(&descriptors);
    let (mel_norm, mel_norm_fit) = normalize_dataset(&melbands_data);

    // Robust scale descriptors (IQR)
    let desc_robust_fit = robustscale_fit(&descriptors);
    let mel_robust_fit = robustscale_fit(&melbands_data);
    let desc_scaled = robustscale_apply(&descriptors, &desc_robust_fit);
    let mel_scaled = robustscale_apply(&melbands_data, &mel_robust_fit);

    // Path relative to the output JSON file's location
    let out_path_buf = PathBuf::from(track::shellexpand(output));
    let out_parent = out_path_buf.parent().unwrap_or(Path::new("."));
    let rel_path = if let Ok(canon_dir) = std::fs::canonicalize(&dir) {
        if let Ok(canon_out) = std::fs::canonicalize(out_parent) {
            pathdiff(&canon_dir, &canon_out)
        } else {
            dir.file_name().unwrap_or_default().to_string_lossy().to_string() + "/"
        }
    } else {
        dir.file_name().unwrap_or_default().to_string_lossy().to_string() + "/"
    };

    // Build the JSON
    let corpus_json = json!({
        "meta": {
            "header": "Corpus Sampler Analysis",
            "info": {
                "artist": "",
                "title": rel_path.trim_end_matches('/'),
                "description": format!("Exported by mr export --sp-tools ({} grains)", wavs.len()),
                "date_created": "",
                "date_analyzed": chrono_now(),
                "url": "",
                "comment": ""
            },
            "file": {
                "path": rel_path,
                "num_files": wavs.len(),
                "sample_rates": sample_rate,
                "min_duration": min_duration_ms,
                "mean_duration": total_duration_ms / n,
                "mean_time_centroid": sum_time_centroid / n,
                "max_duration": max_duration_ms,
                "mean_loudness": sum_loudness / n,
                "mean_centroid": sum_centroid / n,
                "mean_flatness": sum_flatness / n,
                "mean_pitch": sum_pitch / n,
                "mean_pitch_confidence": sum_pitch_conf / n
            },
            "setttings": {
                "fftsettings": "default FluCoMa settings",
                "numframes": "whole sample",
                "descriptors": "mean of loudness, deriv of loudness, mean of centroid, deriv of centroid, mean of flatness, deriv of flatness, mean of pitch, pitch confidence, 40 melbands",
                "comment": "exported by mr export --sp-tools"
            }
        },
        "data": {
            "datasets": {
                "descriptors_256": dataset_json(&descriptors, 8),
                "descriptors_4410": dataset_json(&descriptors, 8),
                "descriptors_all": dataset_json(&descriptors, 8),
                "melbands_256": dataset_json(&melbands_data, 40),
                "melbands_4410": dataset_json(&melbands_data, 40),
                "melbands_all": dataset_json(&melbands_data, 40),
            },
            "normalized_datasets": {
                "descriptors_256": dataset_json(&desc_norm, 8),
                "descriptors_4410": dataset_json(&desc_norm, 8),
                "descriptors_all": dataset_json(&desc_norm, 8),
                "melbands_256": dataset_json(&mel_norm, 40),
                "melbands_4410": dataset_json(&mel_norm, 40),
                "melbands_all": dataset_json(&mel_norm, 40),
            },
            "scaled_datasets": {
                "descriptors_256": dataset_json(&desc_scaled, 8),
                "descriptors_4410": dataset_json(&desc_scaled, 8),
                "descriptors_all": dataset_json(&desc_scaled, 8),
                "melbands_256": dataset_json(&mel_scaled, 40),
                "melbands_4410": dataset_json(&mel_scaled, 40),
                "melbands_all": dataset_json(&mel_scaled, 40),
            },
            "coll": coll_data
        },
        "fits": {
            "normalized": {
                "descriptors_256": desc_norm_fit.clone(),
                "descriptors_4410": desc_norm_fit.clone(),
                "descriptors_all": desc_norm_fit,
                "melbands_256": mel_norm_fit.clone(),
                "melbands_4410": mel_norm_fit.clone(),
                "melbands_all": mel_norm_fit,
            },
            "robustscaled": {
                "descriptors_256": desc_robust_fit.clone(),
                "descriptors_4410": desc_robust_fit.clone(),
                "descriptors_all": desc_robust_fit,
                "melbands_256": mel_robust_fit.clone(),
                "melbands_4410": mel_robust_fit.clone(),
                "melbands_all": mel_robust_fit,
            }
        }
    });

    let out_path = PathBuf::from(track::shellexpand(output));
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json_str = serde_json::to_string_pretty(&corpus_json)?;
    std::fs::write(&out_path, json_str)?;

    eprintln!("exported SP-Tools corpus → {}", out_path.display());
    Ok(())
}

// ── Export helpers ───────────────────────────────────────────────────

enum AnalysisKind {
    Loudness,
    Centroid,
    Flatness,
}

/// Analyze a file and return (mean, derivative) for a given descriptor.
fn analyze_with_deriv(path: &Path, config: &Config, kind: AnalysisKind) -> (f64, f64) {
    match kind {
        AnalysisKind::Loudness => {
            match corpus::flucoma::analyze::loudness(path, config) {
                Ok(frames) if frames.len() > 1 => {
                    let vals: Vec<f64> = frames.iter().map(|f| f.loudness).collect();
                    (mean(&vals), mean_derivative(&vals))
                }
                Ok(frames) if !frames.is_empty() => (frames[0].loudness, 0.0),
                _ => (0.0, 0.0),
            }
        }
        AnalysisKind::Centroid | AnalysisKind::Flatness => {
            match corpus::flucoma::analyze::spectral_shape(path, config) {
                Ok(frames) if frames.len() > 1 => {
                    let vals: Vec<f64> = frames.iter().map(|f| match kind {
                        AnalysisKind::Centroid => f.centroid,
                        AnalysisKind::Flatness => f.flatness,
                        _ => 0.0,
                    }).collect();
                    (mean(&vals), mean_derivative(&vals))
                }
                Ok(frames) if !frames.is_empty() => {
                    let v = match kind {
                        AnalysisKind::Centroid => frames[0].centroid,
                        AnalysisKind::Flatness => frames[0].flatness,
                        _ => 0.0,
                    };
                    (v, 0.0)
                }
                _ => (0.0, 0.0),
            }
        }
    }
}

/// Analyze pitch: return (mean_pitch_hz, mean_confidence).
fn analyze_pitch_summary(path: &Path, config: &Config) -> (f64, f64) {
    match corpus::flucoma::analyze::pitch(path, config) {
        Ok(frames) if !frames.is_empty() => {
            let n = frames.len() as f64;
            let mean_hz: f64 = frames.iter().map(|f| f.hz).sum::<f64>() / n;
            let mean_conf: f64 = frames.iter().map(|f| f.confidence).sum::<f64>() / n;
            (mean_hz, mean_conf)
        }
        _ => (0.0, 0.0),
    }
}

fn mean(vals: &[f64]) -> f64 {
    if vals.is_empty() { return 0.0; }
    vals.iter().sum::<f64>() / vals.len() as f64
}

fn mean_derivative(vals: &[f64]) -> f64 {
    if vals.len() < 2 { return 0.0; }
    let derivs: Vec<f64> = vals.windows(2).map(|w| w[1] - w[0]).collect();
    mean(&derivs)
}

fn dataset_json(data: &BTreeMap<String, Vec<f64>>, cols: usize) -> Value {
    let data_obj: BTreeMap<&str, &Vec<f64>> = data.iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    json!({ "cols": cols, "data": data_obj })
}

/// Min-max normalize a dataset to 0..1. Returns (normalized_data, fit_json).
fn normalize_dataset(data: &BTreeMap<String, Vec<f64>>) -> (BTreeMap<String, Vec<f64>>, Value) {
    if data.is_empty() {
        return (BTreeMap::new(), json!({}));
    }
    let dims = data.values().next().unwrap().len();
    let mut mins = vec![f64::MAX; dims];
    let mut maxs = vec![f64::MIN; dims];

    for v in data.values() {
        for (i, &val) in v.iter().enumerate() {
            if i < dims {
                mins[i] = mins[i].min(val);
                maxs[i] = maxs[i].max(val);
            }
        }
    }

    let mut normed = BTreeMap::new();
    for (k, v) in data {
        let nv: Vec<f64> = v.iter().enumerate().map(|(i, &val)| {
            let range = maxs[i] - mins[i];
            if range > 1e-12 { (val - mins[i]) / range } else { 0.0 }
        }).collect();
        normed.insert(k.clone(), nv);
    }

    let fit = json!({
        "cols": dims,
        "data_min": mins,
        "data_max": maxs,
        "min": 0.0,
        "max": 1.0
    });

    (normed, fit)
}

/// Compute robust scaling fit (median, IQR at 25/75 percentiles).
fn robustscale_fit(data: &BTreeMap<String, Vec<f64>>) -> Value {
    if data.is_empty() {
        return json!({});
    }
    let dims = data.values().next().unwrap().len();
    let n = data.len();

    let mut columns: Vec<Vec<f64>> = vec![Vec::with_capacity(n); dims];
    for v in data.values() {
        for (i, &val) in v.iter().enumerate() {
            if i < dims {
                columns[i].push(val);
            }
        }
    }

    let mut medians = vec![0.0; dims];
    let mut lows = vec![0.0; dims];
    let mut highs = vec![0.0; dims];
    let mut ranges = vec![0.0; dims];

    for (i, col) in columns.iter_mut().enumerate() {
        col.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        medians[i] = percentile(col, 50.0);
        lows[i] = percentile(col, 25.0);
        highs[i] = percentile(col, 75.0);
        ranges[i] = highs[i] - lows[i];
    }

    json!({
        "cols": dims,
        "data_low": lows,
        "data_high": highs,
        "low": 25.0,
        "high": 75.0,
        "median": medians,
        "range": ranges
    })
}

/// Apply robust scaling: (val - median) / range.
fn robustscale_apply(data: &BTreeMap<String, Vec<f64>>, fit: &Value) -> BTreeMap<String, Vec<f64>> {
    let medians = fit["median"].as_array().unwrap();
    let ranges = fit["range"].as_array().unwrap();

    let mut scaled = BTreeMap::new();
    for (k, v) in data {
        let sv: Vec<f64> = v.iter().enumerate().map(|(i, &val)| {
            let med = medians.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0);
            let rng = ranges.get(i).and_then(|v| v.as_f64()).unwrap_or(1.0);
            if rng.abs() > 1e-12 { (val - med) / rng } else { 0.0 }
        }).collect();
        scaled.insert(k.clone(), sv);
    }
    scaled
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    let idx = (p / 100.0) * (sorted.len() - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi || hi >= sorted.len() {
        sorted[lo.min(sorted.len() - 1)]
    } else {
        let frac = idx - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

fn chrono_now() -> String {
    "exported by mr".to_string()
}

/// Compute relative path from `base` to `target`, with trailing slash.
fn pathdiff(target: &Path, base: &Path) -> String {
    if target == base {
        return "./".to_string();
    }
    // Try to strip base prefix
    if let Ok(rel) = target.strip_prefix(base) {
        let mut s = rel.to_string_lossy().to_string();
        if !s.ends_with('/') {
            s.push('/');
        }
        return s;
    }
    // Fallback: just use the target dir name
    let mut s = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    if !s.ends_with('/') {
        s.push('/');
    }
    s
}
