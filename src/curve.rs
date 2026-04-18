use std::f64::consts::PI;

use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};

use crate::error::Result;
use crate::json::{MrData, write_stdout};

/// Generate a sine wave curve.
pub fn sine(cycle: f64, range: [f64; 2], bars: f64, phase: f64, resolution: f64) -> Result<()> {
    let [vmin, vmax] = range;
    let mut points = Vec::new();
    let mut t = 0.0;
    while t <= bars {
        let n = 0.5 + 0.5 * (2.0 * PI * (t / cycle + phase)).sin();
        points.push([t, vmin + n * (vmax - vmin)]);
        t += resolution;
    }
    write_stdout(&MrData::Curve { points })
}

/// Generate a drunk walk curve (Brownian motion with optional gravity).
pub fn drunk(
    range: [f64; 2],
    bars: f64,
    step: f64,
    gravity: f64,
    seed: u64,
    resolution: f64,
) -> Result<()> {
    let [vmin, vmax] = range;
    let center = (vmin + vmax) / 2.0;
    let mut rng = SmallRng::seed_from_u64(seed);
    let mut val = center;
    let mut points = Vec::new();
    let mut t = 0.0;
    while t <= bars {
        points.push([t, val]);
        // Random step
        val += rng.random::<f64>() * step * 2.0 - step;
        // Gravity toward center
        if gravity > 0.0 {
            val += (center - val) * gravity * 0.1;
        }
        val = val.clamp(vmin, vmax);
        t += resolution;
    }
    write_stdout(&MrData::Curve { points })
}

/// Generate an ASR envelope curve.
pub fn envelope(
    attack: f64,
    sustain: f64,
    release: f64,
    bars: f64,
    range: [f64; 2],
    resolution: f64,
) -> Result<()> {
    let [vmin, vmax] = range;
    let mut points = Vec::new();
    let mut t = 0.0;
    let attack_end = attack * bars;
    let release_start = bars - release * bars;

    while t <= bars {
        let v = if t < attack_end {
            // Attack: ramp up
            t / attack_end
        } else if t < release_start {
            // Sustain
            sustain
        } else {
            // Release: ramp down from sustain
            sustain * (1.0 - (t - release_start) / (bars - release_start))
        };
        points.push([t, vmin + v * (vmax - vmin)]);
        t += resolution;
    }
    write_stdout(&MrData::Curve { points })
}

/// Generate a linear ramp curve.
pub fn ramp(start: f64, end: f64, bars: f64, resolution: f64) -> Result<()> {
    let mut points = Vec::new();
    let mut t = 0.0;
    while t <= bars {
        let frac = t / bars;
        points.push([t, start + frac * (end - start)]);
        t += resolution;
    }
    write_stdout(&MrData::Curve { points })
}

/// Generate a stochastic curve (random values with smoothing).
pub fn stochastic(
    range: [f64; 2],
    bars: f64,
    density: usize,
    smoothing: usize,
    seed: u64,
    resolution: f64,
) -> Result<()> {
    let [vmin, vmax] = range;
    let mut rng = SmallRng::seed_from_u64(seed);

    // Generate raw random values
    let num_points = ((bars / resolution) as usize).max(1);
    let mut raw: Vec<f64> = Vec::new();
    for _ in 0..num_points {
        // density controls how many random values influence each point
        let mut sum = 0.0;
        for _ in 0..density {
            sum += rng.random::<f64>();
        }
        raw.push(sum / density as f64);
    }

    // Moving average smoothing
    if smoothing > 1 {
        let mut smoothed = Vec::new();
        for i in 0..raw.len() {
            let start = i.saturating_sub(smoothing / 2);
            let end = (i + smoothing / 2 + 1).min(raw.len());
            let avg: f64 = raw[start..end].iter().sum::<f64>() / (end - start) as f64;
            smoothed.push(avg);
        }
        raw = smoothed;
    }

    let points: Vec<[f64; 2]> = raw
        .into_iter()
        .enumerate()
        .map(|(i, v)| [i as f64 * resolution, vmin + v * (vmax - vmin)])
        .collect();

    write_stdout(&MrData::Curve { points })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sine_range() {
        let mut points = Vec::new();
        let (vmin, vmax) = (0.2, 0.8);
        let mut t = 0.0;
        while t <= 8.0 {
            let n = 0.5 + 0.5 * (2.0 * PI * (t / 4.0)).sin();
            points.push(vmin + n * (vmax - vmin));
            t += 0.5;
        }
        for &v in &points {
            assert!(v >= vmin - 0.001 && v <= vmax + 0.001);
        }
    }

    #[test]
    fn test_drunk_stays_in_range() {
        let (vmin, vmax) = (0.2, 0.8);
        let center = (vmin + vmax) / 2.0;
        let mut rng = SmallRng::seed_from_u64(42);
        let mut val = center;
        for _ in 0..1000 {
            val += rng.random::<f64>() * 0.1 - 0.05;
            val += (center - val) * 0.1 * 0.1;
            val = val.clamp(vmin, vmax);
            assert!(val >= vmin && val <= vmax);
        }
    }

    #[test]
    fn test_envelope_shape() {
        let mut points = Vec::new();
        let bars = 16.0;
        let (attack, sustain, release) = (0.1f64, 0.8f64, 0.3f64);
        let attack_end = attack * bars;
        let release_start = bars - release * bars;
        let mut t: f64 = 0.0;
        while t <= bars {
            let v = if t < attack_end {
                t / attack_end
            } else if t < release_start {
                sustain
            } else {
                sustain * (1.0 - (t - release_start) / (bars - release_start))
            };
            points.push(v);
            t += 0.5;
        }
        // First point should be near 0
        assert!(points[0] < 0.1);
        // Middle points should be near sustain
        let mid = points.len() / 2;
        assert!((points[mid] - sustain).abs() < 0.1f64);
        // Last point should be near 0
        assert!(*points.last().unwrap() < 0.1);
    }

    #[test]
    fn test_ramp_endpoints() {
        let bars = 8.0;
        let (start, end) = (0.2f64, 0.9f64);
        let first = start + 0.0 / bars * (end - start);
        let last = start + 1.0 * (end - start);
        assert!((first - 0.2f64).abs() < 0.001);
        assert!((last - 0.9f64).abs() < 0.001);
    }

    #[test]
    fn test_stochastic_range() {
        let (vmin, vmax) = (0.2, 0.8);
        let mut rng = SmallRng::seed_from_u64(42);
        let num_points = 100usize;
        let density = 3usize;
        let mut raw: Vec<f64> = Vec::new();
        for _ in 0..num_points {
            let mut sum = 0.0;
            for _ in 0..density {
                sum += rng.random::<f64>();
            }
            raw.push(sum / density as f64);
        }
        let points: Vec<f64> = raw.iter().map(|&v| vmin + v * (vmax - vmin)).collect();
        for &v in &points {
            assert!(v >= vmin - 0.001 && v <= vmax + 0.001,
                "stochastic value {} out of range [{}, {}]", v, vmin, vmax);
        }
    }

    #[test]
    fn test_stochastic_smoothing() {
        let mut rng = SmallRng::seed_from_u64(99);
        let num_points = 50usize;
        let density = 1usize;
        let smoothing = 5usize;
        let mut raw: Vec<f64> = Vec::new();
        for _ in 0..num_points {
            let mut sum = 0.0;
            for _ in 0..density { sum += rng.random::<f64>(); }
            raw.push(sum / density as f64);
        }
        // Apply smoothing
        let mut smoothed = Vec::new();
        for i in 0..raw.len() {
            let start = i.saturating_sub(smoothing / 2);
            let end = (i + smoothing / 2 + 1).min(raw.len());
            let avg: f64 = raw[start..end].iter().sum::<f64>() / (end - start) as f64;
            smoothed.push(avg);
        }
        // Smoothed values should have less variance
        let raw_var = variance(&raw);
        let smooth_var = variance(&smoothed);
        assert!(smooth_var <= raw_var, "smoothing should reduce variance");
    }

    #[test]
    fn test_stochastic_density_affects_distribution() {
        // Higher density → values cluster toward 0.5 (central limit theorem)
        let mut rng_low = SmallRng::seed_from_u64(42);
        let mut rng_high = SmallRng::seed_from_u64(42);
        let n = 200;

        let low_density: Vec<f64> = (0..n).map(|_| rng_low.random::<f64>()).collect();
        let high_density: Vec<f64> = (0..n).map(|_| {
            let sum: f64 = (0..10).map(|_| rng_high.random::<f64>()).sum();
            sum / 10.0
        }).collect();

        let low_var = variance(&low_density);
        let high_var = variance(&high_density);
        assert!(high_var < low_var, "higher density should reduce variance");
    }

    fn variance(data: &[f64]) -> f64 {
        let mean = data.iter().sum::<f64>() / data.len() as f64;
        data.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / data.len() as f64
    }
}
