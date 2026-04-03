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

    // Path in Mac HFS format (colon-separated) as SP-Tools expects
    let canon_dir = std::fs::canonicalize(&dir)
        .unwrap_or_else(|_| dir.to_path_buf());
    let rel_path = posix_to_hfs(&canon_dir);

    // Build zero-filled stub datasets for mfccs (104 cols) and sines (64 cols)
    let keys: Vec<&String> = descriptors.keys().collect();
    let mfcc_stub: BTreeMap<String, Vec<f64>> = keys.iter()
        .map(|k| ((*k).clone(), vec![0.0; 104]))
        .collect();
    let sines_stub: BTreeMap<String, Vec<f64>> = keys.iter()
        .map(|k| ((*k).clone(), vec![0.0; 64]))
        .collect();
    let (mfcc_norm, mfcc_norm_fit) = normalize_dataset(&mfcc_stub);
    let mfcc_robust_fit = robustscale_fit(&mfcc_stub);
    let mfcc_scaled = robustscale_apply(&mfcc_stub, &mfcc_robust_fit);

    // Build the JSON
    let corpus_json = json!({
        "meta": {
            "header": "Corpus Sampler Analysis",
            "info": {
                "artist": "",
                "title": rel_path.trim_end_matches('/'),
                "description": format!("Exported by mr export ({} grains)", wavs.len()),
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
                "descriptors": "mean of loudness, deriv of loudness, mean of centroid, deriv of centroid, mean of flatness, deriv of flatness, mean of pitch, pitch confidence, 40 melbands, 104 mfccs, 64 sines",
                "comment": "exported by mr export"
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
                "mfccs_256": dataset_json(&mfcc_stub, 104),
                "mfccs_4410": dataset_json(&mfcc_stub, 104),
                "mfccs_all": dataset_json(&mfcc_stub, 104),
                "sines_256": {},
                "sines_4410": dataset_json(&sines_stub, 64),
                "sines_all": dataset_json(&sines_stub, 64),
            },
            "normalized_datasets": {
                "descriptors_256": dataset_json(&desc_norm, 8),
                "descriptors_4410": dataset_json(&desc_norm, 8),
                "descriptors_all": dataset_json(&desc_norm, 8),
                "melbands_256": dataset_json(&mel_norm, 40),
                "melbands_4410": dataset_json(&mel_norm, 40),
                "melbands_all": dataset_json(&mel_norm, 40),
                "mfccs_256": dataset_json(&mfcc_norm, 104),
                "mfccs_4410": dataset_json(&mfcc_norm, 104),
                "mfccs_all": dataset_json(&mfcc_norm, 104),
            },
            "scaled_datasets": {
                "descriptors_256": dataset_json(&desc_scaled, 8),
                "descriptors_4410": dataset_json(&desc_scaled, 8),
                "descriptors_all": dataset_json(&desc_scaled, 8),
                "melbands_256": dataset_json(&mel_scaled, 40),
                "melbands_4410": dataset_json(&mel_scaled, 40),
                "melbands_all": dataset_json(&mel_scaled, 40),
                "mfccs_256": dataset_json(&mfcc_scaled, 104),
                "mfccs_4410": dataset_json(&mfcc_scaled, 104),
                "mfccs_all": dataset_json(&mfcc_scaled, 104),
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
                "mfccs_256": mfcc_norm_fit.clone(),
                "mfccs_4410": mfcc_norm_fit.clone(),
                "mfccs_all": mfcc_norm_fit,
            },
            "robustscaled": {
                "descriptors_256": desc_robust_fit.clone(),
                "descriptors_4410": desc_robust_fit.clone(),
                "descriptors_all": desc_robust_fit,
                "melbands_256": mel_robust_fit.clone(),
                "melbands_4410": mel_robust_fit.clone(),
                "melbands_all": mel_robust_fit,
                "mfccs_256": mfcc_robust_fit.clone(),
                "mfccs_4410": mfcc_robust_fit.clone(),
                "mfccs_all": mfcc_robust_fit,
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

// ── mr corpora ──────────────────────────────────────────────────────

pub fn corpora_list() -> Result<()> {
    let names = corpus::persist::list()?;
    if names.is_empty() {
        eprintln!("no saved corpora (use mr analyze to create one)");
    } else {
        for name in &names {
            let c = corpus::persist::load(name)?;
            eprintln!("  {:<20} {} grains", name, c.len());
        }
    }
    Ok(())
}

pub fn corpus_info(name: &str) -> Result<()> {
    let c = corpus::persist::load(name)?;
    eprintln!("corpus: {}", name);
    eprintln!("grains: {}", c.len());
    if c.len() > 0 {
        let dims = c.grains()[0].features.dim();
        eprintln!("dimensions: {}", dims);

        // Show feature stats
        let features: Vec<corpus::Features> = c.grains().iter().map(|g| g.features.clone()).collect();
        let stats = corpus::stats::summarize(&features);
        eprintln!("features:");
        for (i, s) in stats.iter().enumerate() {
            eprintln!("  dim {:<3} mean={:>8.2}  std={:>8.2}  min={:>8.2}  max={:>8.2}",
                i, s.mean, s.stddev, s.min, s.max);
        }

        // Show duration range
        let durs: Vec<f64> = c.grains().iter().map(|g| g.duration).collect();
        let min_dur = durs.iter().cloned().fold(f64::MAX, f64::min);
        let max_dur = durs.iter().cloned().fold(0.0f64, f64::max);
        eprintln!("duration: {:.3}s — {:.3}s", min_dur, max_dur);
    }
    Ok(())
}

pub fn corpus_query(
    name: &str,
    n: usize,
    sort: Option<&str>,
    like: Option<&str>,
) -> Result<()> {
    let c = corpus::persist::load(name)?;
    if c.is_empty() {
        return Err(Error::Other(format!("corpus '{}' is empty", name)));
    }

    let results: Vec<&corpus::Grain> = if let Some(audio_path) = like {
        // Query by similarity to an audio file
        let path = resolve_audio(audio_path)?;
        let config = Config::new();
        let mut feature_values = Vec::new();

        // Extract same features as analyze
        if let Ok(frames) = corpus::flucoma::analyze::mfcc(&path, 13, &config) {
            if !frames.is_empty() {
                let n_frames = frames.len() as f64;
                let dims = frames[0].len();
                for d in 1..dims {
                    let mean: f64 = frames.iter().map(|f| f[d]).sum::<f64>() / n_frames;
                    feature_values.push(mean);
                }
            }
        }
        if let Ok(frames) = corpus::flucoma::analyze::spectral_shape(&path, &config) {
            if !frames.is_empty() {
                let n_frames = frames.len() as f64;
                feature_values.push(frames.iter().map(|f| f.centroid).sum::<f64>() / n_frames);
                feature_values.push(frames.iter().map(|f| f.spread).sum::<f64>() / n_frames);
                feature_values.push(frames.iter().map(|f| f.flatness).sum::<f64>() / n_frames);
            }
        }
        if let Ok(frames) = corpus::flucoma::analyze::pitch(&path, &config) {
            if !frames.is_empty() {
                let confident: Vec<f64> = frames.iter()
                    .filter(|f| f.confidence > 0.5)
                    .map(|f| f.hz)
                    .collect();
                feature_values.push(if confident.is_empty() { 0.0 } else {
                    let mut s = confident.clone();
                    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    s[s.len() / 2]
                });
            }
        }
        if let Ok(frames) = corpus::flucoma::analyze::loudness(&path, &config) {
            if !frames.is_empty() {
                let n_frames = frames.len() as f64;
                feature_values.push(frames.iter().map(|f| f.loudness).sum::<f64>() / n_frames);
            }
        }

        // Pad or truncate to match corpus dimensions
        let target_dim = c.grains()[0].features.dim();
        feature_values.resize(target_dim, 0.0);

        // Normalize using corpus params if available
        let mut target = corpus::Features::new(feature_values);
        if let Some(params) = c.norm_params() {
            params.apply(&mut target);
        }

        c.query(&target, n)
    } else if let Some(sort_key) = sort {
        // Sort by a named feature dimension
        let mut grains: Vec<&corpus::Grain> = c.grains().iter().collect();
        let reverse = !sort_key.starts_with('-');
        let key = sort_key.trim_start_matches('-');

        // Map friendly names to dimension indices
        // Our analyze produces: 12 MFCCs (dims 0-11), centroid (12), spread (13), flatness (14), pitch (15), loudness (16)
        let dim = match key {
            "bright" | "centroid" => Some(12),
            "dark" => { grains.sort_by(|a, b| a.features.get(12).partial_cmp(&b.features.get(12)).unwrap()); Some(usize::MAX) }, // handled
            "loud" | "loudness" => Some(16),
            "quiet" => { grains.sort_by(|a, b| a.features.get(16).partial_cmp(&b.features.get(16)).unwrap()); Some(usize::MAX) },
            "high" | "pitch" => Some(15),
            "low" => { grains.sort_by(|a, b| a.features.get(15).partial_cmp(&b.features.get(15)).unwrap()); Some(usize::MAX) },
            "noisy" | "flatness" => Some(14),
            "duration" | "long" => {
                grains.sort_by(|a, b| b.duration.partial_cmp(&a.duration).unwrap());
                Some(usize::MAX)
            }
            "short" => {
                grains.sort_by(|a, b| a.duration.partial_cmp(&b.duration).unwrap());
                Some(usize::MAX)
            }
            d if d.parse::<usize>().is_ok() => Some(d.parse().unwrap()),
            _ => None,
        };

        if let Some(d) = dim {
            if d != usize::MAX {
                if reverse {
                    grains.sort_by(|a, b| b.features.get(d).partial_cmp(&a.features.get(d)).unwrap());
                } else {
                    grains.sort_by(|a, b| a.features.get(d).partial_cmp(&b.features.get(d)).unwrap());
                }
            }
        } else {
            return Err(Error::Other(format!(
                "unknown sort key: '{}'\navailable: bright, dark, loud, quiet, high, low, noisy, long, short, duration, or a dimension number",
                key
            )));
        }

        grains.into_iter().take(n).collect()
    } else {
        // Default: just return first N grains
        c.grains().iter().take(n).collect()
    };

    // Output as MrData::Grains for piping
    let grains: Vec<MrGrain> = results.iter().map(|g| MrGrain {
        path: g.source.clone(),
        index: g.id,
        start: g.start,
        duration: g.duration,
        source: g.source.clone(),
    }).collect();

    for g in &results {
        eprintln!("  {} (dur={:.3}s)", g.source, g.duration);
    }

    json::write_stdout(&MrData::Grains {
        source: name.to_string(),
        sample_rate: 44100,
        grains,
    })?;

    Ok(())
}

pub fn corpus_delete(name: &str) -> Result<()> {
    if corpus::persist::delete(name)? {
        eprintln!("deleted corpus '{}'", name);
    } else {
        eprintln!("corpus '{}' not found", name);
    }
    Ok(())
}

// ── mr corpus-load ──────────────────────────────────────────────────

pub fn corpus_load(name: &str, track_name: &str) -> Result<()> {
    let c = corpus::persist::load(name)?;
    if c.is_empty() {
        return Err(Error::Other(format!("corpus '{}' is empty", name)));
    }

    // Find the grain WAV files. They're typically in chops/ directories.
    let grain_paths = find_grain_files(&c)?;
    if grain_paths.is_empty() {
        return Err(Error::Other(
            "no grain WAV files found — need original grain files from mr slice".into(),
        ));
    }

    eprintln!("combining {} grains into single sample...", grain_paths.len());

    // Build combined WAV in ~/.mr/combined/<name>.wav
    let combined_dir = corpus::persist::corpus_dir()?.join("..").join("combined");
    std::fs::create_dir_all(&combined_dir)?;
    let combined_path = combined_dir.join(format!("{name}.wav"));

    // Convert PathBufs to Path refs for combine_wav
    let refs: Vec<(usize, String, &Path)> = grain_paths
        .iter()
        .map(|(id, src, p)| (*id, src.clone(), p.as_path()))
        .collect();

    let result = corpus::audio::combine_wav(&refs, &combined_path)?;

    eprintln!(
        "combined: {:.1}s, {} grains → {}",
        result.total_duration,
        result.entries.len(),
        combined_path.display()
    );

    // Save the grain→sample_start mapping alongside the combined WAV
    let mapping: Vec<serde_json::Value> = result
        .entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "grain_id": e.grain_id,
                "source": e.source,
                "sample_start": e.sample_start,
                "duration": e.duration,
            })
        })
        .collect();
    let mapping_path = combined_dir.join(format!("{name}_mapping.json"));
    std::fs::write(
        &mapping_path,
        serde_json::to_string_pretty(&mapping).unwrap(),
    )?;

    // Stage the combined sample and create/find the track
    let filename = crate::track::stage_sample(&combined_path.to_string_lossy())?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    let session = crate::connect::connect()?;
    let idx = match crate::connect::resolve_track(&session, track_name) {
        Ok(idx) => idx,
        Err(_) => {
            let track = session.create_midi_track(-1)?;
            std::thread::sleep(std::time::Duration::from_millis(200));
            track.set_name(track_name)?;
            track.track_idx
        }
    };

    let track = session.track(idx);
    // Load by filename (browser search)
    let stem = Path::new(&filename)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let load_result = track.load_sample(&filename);
    if load_result.is_err() {
        track.load_sample(&stem)?;
    }
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Set Simpler Volume to 6dB
    let device = track.device(0);
    let param_names = device.parameter_names()?;
    if let Some(vol_idx) = param_names.iter().position(|n| n == "Volume") {
        device.set_param(vol_idx as i32, 6.0)?;
    }

    eprintln!(
        "loaded corpus '{}' into track '{}' ({} grains, {:.1}s)",
        name,
        track_name,
        result.entries.len(),
        result.total_duration,
    );
    eprintln!("mapping saved: {}", mapping_path.display());

    Ok(())
}

/// Find grain WAV files for a corpus by searching likely directories.
fn find_grain_files(
    c: &corpus::Corpus,
) -> Result<Vec<(usize, String, PathBuf)>> {
    let mut paths = Vec::new();

    // Collect unique source filenames and their grain IDs
    for grain in c.grains() {
        let source = &grain.source;

        // Try to find the file in likely locations
        let candidates = [
            PathBuf::from(source),                           // absolute or relative
            PathBuf::from("chops").join(source),             // chops/<file>
        ];

        // Also search chops/*/<source> for subdirectories
        let mut found = None;
        for candidate in &candidates {
            if candidate.exists() {
                found = Some(candidate.clone());
                break;
            }
        }

        // Search chops subdirectories
        if found.is_none() {
            if let Ok(entries) = std::fs::read_dir("chops") {
                for entry in entries.flatten() {
                    if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        let sub = entry.path().join(source);
                        if sub.exists() {
                            found = Some(sub);
                            break;
                        }
                    }
                }
            }
        }

        if let Some(path) = found {
            paths.push((grain.id, grain.source.clone(), path));
        }
    }

    Ok(paths)
}

// ── mr concat ───────────────────────────────────────────────────────

pub fn concat(
    source_audio: &str,
    corpus_name: &str,
    target: &str,
    method: &str,
    threshold: Option<f64>,
    candidates: usize,
    seed: Option<u64>,
    bpm: Option<f64>,
) -> Result<()> {
    let source_path = resolve_audio(source_audio)?;
    let c = corpus::persist::load(corpus_name)?;
    if c.is_empty() {
        return Err(Error::Other(format!("corpus '{}' is empty", corpus_name)));
    }

    // Get tempo for beat conversion
    let tempo = if let Some(b) = bpm {
        b
    } else {
        let session = crate::connect::connect()?;
        session.get_tempo()? as f64
    };
    let secs_per_beat = 60.0 / tempo;

    // Step 1: Slice source audio
    eprintln!("slicing source ({})...", method);
    let config = corpus::flucoma::Config::new();
    let info = corpus::audio::wav_info(&source_path)?;

    let slice_points = match method {
        "onset" => {
            let mut opts = corpus::flucoma::slice::OnsetOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::onset_slice(&source_path, &opts, &config)?
        }
        "novelty" => {
            let mut opts = corpus::flucoma::slice::NoveltyOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::novelty_slice(&source_path, &opts, &config)?
        }
        "amp" => {
            let mut opts = corpus::flucoma::slice::AmpOpts::default();
            if let Some(t) = threshold {
                opts.on_threshold = t;
            }
            corpus::flucoma::slice::amp_slice(&source_path, &opts, &config)?
        }
        "transient" => {
            let mut opts = corpus::flucoma::slice::TransientOpts::default();
            if let Some(t) = threshold {
                opts.threshold = t;
            }
            corpus::flucoma::slice::transient_slice(&source_path, &opts, &config)?
        }
        other => return Err(Error::Other(format!("unknown slice method: {other}"))),
    };

    // Build onset boundaries in seconds
    let sr = info.sample_rate as f64;
    let total_secs = info.num_frames as f64 / sr;
    let mut boundaries_secs: Vec<f64> = vec![0.0];
    for &p in &slice_points {
        boundaries_secs.push(p as f64 / sr);
    }
    boundaries_secs.push(total_secs);
    boundaries_secs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    boundaries_secs.dedup();

    eprintln!("  {} segments", boundaries_secs.len() - 1);

    // Step 2: Split source into temp grains and extract features
    let temp_dir = tempfile::tempdir()?;
    let temp_out = temp_dir.path().join("segments");

    let frame_points: Vec<usize> = slice_points.clone();
    let splits = corpus::audio::split_wav(&source_path, &frame_points, &temp_out, 0)?;

    eprintln!("extracting features from {} segments...", splits.len());

    let mut segments: Vec<(f64, f64, corpus::Features)> = Vec::new();

    for (i, split) in splits.iter().enumerate() {
        let onset_secs = split.start_sample as f64 / sr;
        let dur_secs = split.duration_secs;

        let mut feature_values: Vec<f64> = Vec::new();

        // MFCC (12 coefficients, skip coeff 0)
        if let Ok(frames) = corpus::flucoma::analyze::mfcc(&split.path, 13, &config) {
            if !frames.is_empty() {
                let n = frames.len() as f64;
                let dims = frames[0].len();
                for d in 1..dims {
                    let mean: f64 = frames.iter().map(|f| f[d]).sum::<f64>() / n;
                    feature_values.push(mean);
                }
            }
        }
        if feature_values.is_empty() {
            feature_values.extend_from_slice(&[0.0; 12]);
        }

        // Spectral shape (centroid, spread, flatness)
        if let Ok(frames) = corpus::flucoma::analyze::spectral_shape(&split.path, &config) {
            if !frames.is_empty() {
                let n = frames.len() as f64;
                feature_values.push(frames.iter().map(|f| f.centroid).sum::<f64>() / n);
                feature_values.push(frames.iter().map(|f| f.spread).sum::<f64>() / n);
                feature_values.push(frames.iter().map(|f| f.flatness).sum::<f64>() / n);
            } else {
                feature_values.extend_from_slice(&[0.0; 3]);
            }
        } else {
            feature_values.extend_from_slice(&[0.0; 3]);
        }

        // Pitch
        if let Ok(frames) = corpus::flucoma::analyze::pitch(&split.path, &config) {
            let confident: Vec<f64> = frames
                .iter()
                .filter(|f| f.confidence > 0.5)
                .map(|f| f.hz)
                .collect();
            let pitch = if confident.is_empty() {
                0.0
            } else {
                let mut sorted = confident;
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                sorted[sorted.len() / 2]
            };
            feature_values.push(pitch);
        } else {
            feature_values.push(0.0);
        }

        // Loudness
        if let Ok(frames) = corpus::flucoma::analyze::loudness(&split.path, &config) {
            if !frames.is_empty() {
                let n = frames.len() as f64;
                feature_values.push(frames.iter().map(|f| f.loudness).sum::<f64>() / n);
            } else {
                feature_values.push(0.0);
            }
        } else {
            feature_values.push(0.0);
        }

        // Pad/truncate to match corpus dimensions
        let target_dim = c.grains()[0].features.dim();
        feature_values.resize(target_dim, 0.0);

        let mut features = corpus::Features::new(feature_values);
        if let Some(params) = c.norm_params() {
            params.apply(&mut features);
        }

        eprint!("\r  {}/{}", i + 1, splits.len());
        segments.push((onset_secs, dur_secs, features));
    }
    eprintln!();

    // Step 3: Mosaic match
    eprintln!("matching against corpus '{}'...", corpus_name);
    let opts = corpus::concat::MosaicOpts {
        candidates,
        seed,
        weights: None,
    };
    let matches = corpus::concat::mosaic(&segments, &c, &opts);

    // Report matches
    let avg_dist: f64 = matches.iter().map(|m| m.distance).sum::<f64>() / matches.len() as f64;
    eprintln!(
        "  {} matches, avg distance: {:.3}",
        matches.len(),
        avg_dist
    );

    // Step 4: Load the corpus-load mapping to get grain→sample_start
    let combined_dir = corpus::persist::corpus_dir()?.join("..").join("combined");
    let mapping_path = combined_dir.join(format!("{corpus_name}_mapping.json"));

    if !mapping_path.exists() {
        return Err(Error::Other(format!(
            "no corpus mapping found — run `mr corpus-load {} <track>` first",
            corpus_name
        )));
    }

    let mapping_json = std::fs::read_to_string(&mapping_path)?;
    let mapping: Vec<serde_json::Value> = serde_json::from_str(&mapping_json)?;

    // Build grain_id → sample_start lookup
    let mut grain_start: std::collections::HashMap<usize, f64> = std::collections::HashMap::new();
    for entry in &mapping {
        if let (Some(id), Some(ss)) = (
            entry["grain_id"].as_u64(),
            entry["sample_start"].as_f64(),
        ) {
            grain_start.insert(id as usize, ss);
        }
    }

    // Step 5: Write to Ableton — notes + S Start automation
    let (track_name, slot) = crate::connect::parse_target(target)?;
    let session = crate::connect::connect()?;
    let idx = crate::connect::resolve_track(&session, track_name)?;
    let track = session.track(idx);

    // Calculate clip length in beats
    let total_beats = total_secs / secs_per_beat;
    let clip_length = (total_beats.ceil()).max(1.0);

    // Create the clip
    if track.has_clip(slot)? {
        track.delete_clip(slot)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let clip = track.create_clip(slot, clip_length as f32)?;
    std::thread::sleep(std::time::Duration::from_millis(100));
    clip.set_name(&format!("concat:{corpus_name}"))?;
    clip.set_looping(true)?;

    // Write notes — all at pitch 60 (Simpler default), timed to onsets
    let notes: Vec<ableton::Note> = matches
        .iter()
        .map(|m| {
            let start_beats = m.source_onset / secs_per_beat;
            let dur_beats = (m.source_duration / secs_per_beat).max(0.05);
            ableton::Note::new(60, start_beats as f32, dur_beats as f32, 100)
        })
        .collect();

    for chunk in notes.chunks(80) {
        clip.add_notes(chunk)?;
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // Write S Start automation — set value just before each note
    let device = track.device(0);
    let param_names = device.parameter_names()?;
    let s_start_idx = param_names
        .iter()
        .position(|n| n.contains("S Start") || n.contains("Start"))
        .ok_or_else(|| Error::Other("Simpler 'S Start' parameter not found".into()))?
        as i32;

    // Build automation points: for each match, set S Start to the grain's position
    let mut auto_points: Vec<(f32, f32)> = Vec::new();
    for m in &matches {
        let beat = (m.source_onset / secs_per_beat) as f32;
        let sample_start = grain_start.get(&m.grain_id).copied().unwrap_or(0.0) as f32;

        // Set value slightly before the note triggers
        let pre = (beat - 0.01).max(0.0);
        auto_points.push((pre, sample_start));
        auto_points.push((beat, sample_start));
    }

    if !auto_points.is_empty() {
        clip.automate_smooth(0, s_start_idx, &auto_points, 0.0)?;
    }

    std::thread::sleep(std::time::Duration::from_millis(50));
    clip.fire()?;

    eprintln!(
        "wrote {} notes + S Start automation to {}:{} ({:.1} beats @ {:.0} bpm)",
        notes.len(),
        track_name,
        slot,
        clip_length,
        tempo,
    );

    // Print top matches for interest
    eprintln!("\nmatches:");
    for m in matches.iter().take(10) {
        eprintln!(
            "  {:.2}s → {} (dist: {:.3})",
            m.source_onset, m.grain_source, m.distance
        );
    }
    if matches.len() > 10 {
        eprintln!("  ... and {} more", matches.len() - 10);
    }

    Ok(())
}

fn chrono_now() -> String {
    "exported by mr".to_string()
}

/// Convert a POSIX path to Mac HFS-style colon path.
/// `/Users/foo/bar/` → `Macintosh HD:/Users/foo/bar/`
fn posix_to_hfs(path: &Path) -> String {
    let posix = path.to_string_lossy();
    let mut hfs = format!("Macintosh HD:{}", posix.replace('/', ":"));
    if !hfs.ends_with(':') {
        hfs.push(':');
    }
    hfs
}
