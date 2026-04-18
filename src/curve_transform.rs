use crate::error::Result;
use crate::json::{MrData, read_stdin, write_stdout};

/// Invert a curve (flip values: new_value = 1.0 - value).
pub fn invert() -> Result<()> {
    let data = read_stdin()?;
    let points = data.into_curve("curve-invert")?;
    let result: Vec<[f64; 2]> = points
        .into_iter()
        .map(|[t, v]| [t, 1.0 - v])
        .collect();
    write_stdout(&MrData::Curve { points: result })
}

/// Reverse a curve in time (keep time grid, flip value order).
pub fn reverse() -> Result<()> {
    let data = read_stdin()?;
    let points = data.into_curve("curve-reverse")?;
    if points.is_empty() {
        return write_stdout(&MrData::Curve { points });
    }
    let times: Vec<f64> = points.iter().map(|p| p[0]).collect();
    let mut values: Vec<f64> = points.iter().map(|p| p[1]).collect();
    values.reverse();
    let result: Vec<[f64; 2]> = times
        .into_iter()
        .zip(values)
        .map(|(t, v)| [t, v])
        .collect();
    write_stdout(&MrData::Curve { points: result })
}

/// Remap a curve to a new value range.
pub fn scale(new_min: f64, new_max: f64) -> Result<()> {
    let data = read_stdin()?;
    let points = data.into_curve("curve-scale")?;

    // Find current range
    let (cur_min, cur_max) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p[1]), hi.max(p[1]))
    });

    let result: Vec<[f64; 2]> = if (cur_max - cur_min).abs() < 1e-10 {
        // Flat curve: map to midpoint of new range
        let mid = (new_min + new_max) / 2.0;
        points.into_iter().map(|[t, _]| [t, mid]).collect()
    } else {
        points
            .into_iter()
            .map(|[t, v]| {
                let normalized = (v - cur_min) / (cur_max - cur_min);
                [t, new_min + normalized * (new_max - new_min)]
            })
            .collect()
    };

    write_stdout(&MrData::Curve { points: result })
}

/// Shift all curve values by an offset.
pub fn offset(amount: f64) -> Result<()> {
    let data = read_stdin()?;
    let points = data.into_curve("curve-offset")?;
    let result: Vec<[f64; 2]> = points
        .into_iter()
        .map(|[t, v]| [t, (v + amount).clamp(0.0, 1.0)])
        .collect();
    write_stdout(&MrData::Curve { points: result })
}

/// Crossfade between input curve and a second curve.
pub fn blend(second_json: &str, mix: f64) -> Result<()> {
    let data = read_stdin()?;
    let points_a = data.into_curve("curve-blend")?;

    let second_data: MrData = serde_json::from_str(second_json)
        .map_err(|e| crate::error::Error::Other(format!("bad JSON for second curve: {}", e)))?;
    let points_b = second_data.into_curve("curve-blend")?;

    // Use the shorter length, sample both
    let len = points_a.len().min(points_b.len());
    let result: Vec<[f64; 2]> = (0..len)
        .map(|i| {
            let t = points_a[i][0];
            let va = points_a[i][1];
            let vb = points_b[i][1];
            [t, va * (1.0 - mix) + vb * mix]
        })
        .collect();

    write_stdout(&MrData::Curve { points: result })
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_invert_values() {
        let points = vec![[0.0, 0.2], [1.0, 0.8], [2.0, 0.5]];
        let result: Vec<[f64; 2]> = points
            .into_iter()
            .map(|[t, v]| [t, 1.0 - v])
            .collect();
        assert!((result[0][1] - 0.8).abs() < 0.001);
        assert!((result[1][1] - 0.2).abs() < 0.001);
        assert!((result[2][1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_scale_range() {
        let points = vec![[0.0, 0.0], [1.0, 0.5], [2.0, 1.0]];
        let (new_min, new_max) = (0.3, 0.7);
        let result: Vec<f64> = points
            .iter()
            .map(|p| new_min + p[1] * (new_max - new_min))
            .collect();
        assert!((result[0] - 0.3).abs() < 0.001);
        assert!((result[1] - 0.5).abs() < 0.001);
        assert!((result[2] - 0.7).abs() < 0.001);
    }

    #[test]
    fn test_offset_clamps() {
        let points = vec![[0.0, 0.9], [1.0, 0.1]];
        let amount = 0.2f64;
        let result: Vec<f64> = points
            .iter()
            .map(|p| (p[1] + amount).clamp(0.0f64, 1.0))
            .collect();
        assert!((result[0] - 1.0).abs() < 0.001); // clamped
        assert!((result[1] - 0.3).abs() < 0.001);
    }

    #[test]
    fn test_blend_midpoint() {
        let a = vec![[0.0, 0.0], [1.0, 1.0]];
        let b = vec![[0.0, 1.0], [1.0, 0.0]];
        let mix = 0.5;
        let result: Vec<f64> = (0..2)
            .map(|i| a[i][1] * (1.0 - mix) + b[i][1] * mix)
            .collect();
        assert!((result[0] - 0.5).abs() < 0.001);
        assert!((result[1] - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_reverse_values_flipped() {
        let points = vec![[0.0, 0.1], [1.0, 0.5], [2.0, 0.9]];
        let times: Vec<f64> = points.iter().map(|p| p[0]).collect();
        let mut values: Vec<f64> = points.iter().map(|p| p[1]).collect();
        values.reverse();
        let result: Vec<[f64; 2]> = times.into_iter().zip(values).map(|(t, v)| [t, v]).collect();
        // Times preserved, values reversed
        assert!((result[0][0] - 0.0).abs() < 0.001);
        assert!((result[0][1] - 0.9).abs() < 0.001);
        assert!((result[1][0] - 1.0).abs() < 0.001);
        assert!((result[1][1] - 0.5).abs() < 0.001);
        assert!((result[2][0] - 2.0).abs() < 0.001);
        assert!((result[2][1] - 0.1).abs() < 0.001);
    }

    #[test]
    fn test_reverse_empty() {
        let points: Vec<[f64; 2]> = vec![];
        assert!(points.is_empty());
        // Empty curve reverse should be empty
    }

    #[test]
    fn test_reverse_single_point() {
        let points = vec![[0.0, 0.5]];
        let times: Vec<f64> = points.iter().map(|p| p[0]).collect();
        let mut values: Vec<f64> = points.iter().map(|p| p[1]).collect();
        values.reverse();
        let result: Vec<[f64; 2]> = times.into_iter().zip(values).map(|(t, v)| [t, v]).collect();
        assert_eq!(result.len(), 1);
        assert!((result[0][1] - 0.5).abs() < 0.001);
    }
}
