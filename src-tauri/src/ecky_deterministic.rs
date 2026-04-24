pub fn hash01(x: f64, y: f64, seed: f64) -> f64 {
    let raw = (x * 127.1 + y * 311.7 + seed * 74.7).sin() * 43_758.545_312_3;
    fract01(raw)
}

pub fn hash_signed(x: f64, y: f64, seed: f64) -> f64 {
    hash01(x, y, seed) * 2.0 - 1.0
}

pub fn noise2(x: f64, y: f64, seed: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let xf = x - x0;
    let yf = y - y0;
    let n00 = hash01(x0, y0, seed);
    let n10 = hash01(x0 + 1.0, y0, seed);
    let n01 = hash01(x0, y0 + 1.0, seed);
    let n11 = hash01(x0 + 1.0, y0 + 1.0, seed);
    let sx = smoothstep01(xf);
    let sy = smoothstep01(yf);
    let ix0 = lerp(n00, n10, sx);
    let ix1 = lerp(n01, n11, sx);
    lerp(ix0, ix1, sy).clamp(0.0, 1.0)
}

pub fn fbm2(x: f64, y: f64, seed: f64, octaves: f64, lacunarity: f64, gain: f64) -> f64 {
    let octaves = octaves.round().clamp(1.0, 12.0) as usize;
    let lacunarity = if lacunarity.is_finite() {
        lacunarity.max(0.000_1)
    } else {
        2.0
    };
    let gain = if gain.is_finite() {
        gain.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let mut amplitude = 0.5;
    let mut frequency = 1.0;
    let mut total = 0.0;
    let mut normalizer = 0.0;
    for octave in 0..octaves {
        total += noise2(x * frequency, y * frequency, seed + octave as f64 * 17.0) * amplitude;
        normalizer += amplitude;
        amplitude *= gain;
        frequency *= lacunarity;
    }
    if normalizer <= f64::EPSILON {
        0.0
    } else {
        (total / normalizer).clamp(0.0, 1.0)
    }
}

pub fn cell_distance2(x: f64, y: f64, seed: f64) -> f64 {
    let cx = x.floor();
    let cy = y.floor();
    let mut best = f64::INFINITY;
    for oy in -1..=1 {
        for ox in -1..=1 {
            let gx = cx + ox as f64;
            let gy = cy + oy as f64;
            let px = gx + hash01(gx, gy, seed);
            let py = gy + hash01(gx + 19.19, gy + 7.73, seed + 31.0);
            let dist = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            best = best.min(dist);
        }
    }
    (best / std::f64::consts::SQRT_2).clamp(0.0, 1.0)
}

pub fn voronoi2(x: f64, y: f64, seed: f64) -> f64 {
    (1.0 - cell_distance2(x, y, seed)).clamp(0.0, 1.0)
}

pub fn logistic_scalar2(x: f64, y: f64, seed: f64) -> f64 {
    let mut value = hash01(x, y, seed).clamp(1e-6, 1.0 - 1e-6);
    let rate = 3.57 + hash01(seed, x + y, 11.0) * 0.42;
    for _ in 0..10 {
        value = rate * value * (1.0 - value);
    }
    value.clamp(0.0, 1.0)
}

pub fn henon_scalar2(x: f64, y: f64, seed: f64) -> f64 {
    let mut px = hash_signed(x, y, seed) * 0.65;
    let mut py = hash_signed(y + 17.0, x - 11.0, seed + 3.0) * 0.35;
    let a = 1.22 + hash01(seed, x, 23.0) * 0.18;
    let b = 0.22 + hash01(seed, y, 29.0) * 0.1;
    for _ in 0..8 {
        let next_x = 1.0 - a * px * px + py + (x * 0.017 + y * 0.011).sin() * 0.04;
        let next_y = b * px;
        px = next_x.tanh();
        py = next_y.tanh();
    }
    (px * 0.5 + 0.5).clamp(0.0, 1.0)
}

pub fn ikeda_scalar2(x: f64, y: f64, seed: f64) -> f64 {
    let mut px = hash_signed(x, y, seed) * 0.75;
    let mut py = hash_signed(y, x, seed + 41.0) * 0.75;
    let u = 0.82 + hash01(seed, x - y, 47.0) * 0.09;
    for _ in 0..7 {
        let radius2 = px * px + py * py;
        let t = 0.4 - 6.0 / (1.0 + radius2);
        let next_x = 1.0 + u * (px * t.cos() - py * t.sin()) + (x * 0.013).sin() * 0.03;
        let next_y = u * (px * t.sin() + py * t.cos()) + (y * 0.019).cos() * 0.03;
        px = (next_x * 0.55).tanh();
        py = (next_y * 0.55).tanh();
    }
    ((px + py) * 0.25 + 0.5).clamp(0.0, 1.0)
}

pub fn schwarz_p_scalar(x: f64, y: f64, z: f64) -> f64 {
    ((x.cos() + y.cos() + z.cos()) / 3.0 * 0.5 + 0.5).clamp(0.0, 1.0)
}

pub fn diamond_minimal_scalar(x: f64, y: f64, z: f64) -> f64 {
    let sx = x.sin();
    let sy = y.sin();
    let sz = z.sin();
    let cx = x.cos();
    let cy = y.cos();
    let cz = z.cos();
    let raw = sx * sy * sz + sx * cy * cz + cx * sy * cz + cx * cy * sz;
    (raw / 4.0 * 0.5 + 0.5).clamp(0.0, 1.0)
}

pub fn neovius_scalar(x: f64, y: f64, z: f64) -> f64 {
    let cx = x.cos();
    let cy = y.cos();
    let cz = z.cos();
    let raw = 3.0 * (cx + cy + cz) + 4.0 * cx * cy * cz;
    (raw / 13.0 * 0.5 + 0.5).clamp(0.0, 1.0)
}

pub fn fract01(value: f64) -> f64 {
    let mut result = value.fract();
    if result < 0.0 {
        result += 1.0;
    }
    result.clamp(0.0, 1.0)
}

pub fn smoothstep01(x: f64) -> f64 {
    let t = x.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn lerp(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_helpers_are_deterministic_and_bounded() {
        assert_eq!(hash01(2.0, 3.0, 4.0), hash01(2.0, 3.0, 4.0));
        assert_ne!(hash01(2.0, 3.0, 4.0), hash01(2.0, 3.0, 5.0));
        for value in [
            hash01(2.0, 3.0, 4.0),
            noise2(0.2, 0.8, 4.0),
            fbm2(0.2, 0.8, 4.0, 4.0, 2.0, 0.5),
            voronoi2(0.2, 0.8, 4.0),
            cell_distance2(0.2, 0.8, 4.0),
            logistic_scalar2(0.2, 0.8, 4.0),
            henon_scalar2(0.2, 0.8, 4.0),
            ikeda_scalar2(0.2, 0.8, 4.0),
            schwarz_p_scalar(0.2, 0.8, 4.0),
            diamond_minimal_scalar(0.2, 0.8, 4.0),
            neovius_scalar(0.2, 0.8, 4.0),
        ] {
            assert!((0.0..=1.0).contains(&value), "{value}");
        }
    }
}
