use rayon::prelude::*;

#[derive(Clone)]
pub struct SimParams {
    pub slit_sep: f32,    // slit centre-to-centre distance, in sim units
    pub slit_width: f32,  // width of each slit
    pub wavelength: f32,  // lambda
    pub width: usize,     // pixel columns (x)
    pub height: usize,    // pixel rows (z slices)
}

/// One row of complex field amplitudes at a given z depth.
#[derive(Clone)]
pub struct FieldRow {
    pub re: Vec<f32>,
    pub im: Vec<f32>,
}

/// Precomputed field for every z slice. Recompute when params change.
pub struct FieldCache {
    pub rows: Vec<FieldRow>,  // rows[zi][x]
    pub max_intensity: f32,
    pub params: SimParams,
}

impl FieldCache {
    pub fn compute(p: &SimParams) -> Self {
        let n = p.width;
        let k = std::f32::consts::TAU / p.wavelength;
        let cx = n as f32 / 2.0;
        let c1 = cx - p.slit_sep / 2.0;
        let c2 = cx + p.slit_sep / 2.0;
        let hw = p.slit_width / 2.0;

        // Build aperture mask once
        let aperture: Vec<f32> = (0..n)
            .map(|xi| {
                let xf = xi as f32;
                if (xf - c1).abs() <= hw || (xf - c2).abs() <= hw {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();

        // Compute each z row in parallel
        let rows: Vec<FieldRow> = (0..p.height)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + (zi as f32 / p.height as f32) * p.height as f32;
                let mut re = vec![0.0f32; n];
                let mut im = vec![0.0f32; n];
                for x in 0..n {
                    let xf = x as f32;
                    let (mut sum_re, mut sum_im) = (0.0f32, 0.0f32);
                    for xi in 0..n {
                        if aperture[xi] == 0.0 {
                            continue;
                        }
                        let dx = xf - xi as f32;
                        let r = (dx * dx + z * z).sqrt();
                        let phase = k * r;
                        let inv_sqrt_r = 1.0 / r.sqrt();
                        sum_re += phase.cos() * inv_sqrt_r;
                        sum_im += phase.sin() * inv_sqrt_r;
                    }
                    re[x] = sum_re;
                    im[x] = sum_im;
                }
                FieldRow { re, im }
            })
            .collect();

        let max_intensity = rows
            .iter()
            .flat_map(|row| {
                row.re.iter().zip(row.im.iter()).map(|(r, i)| r * r + i * i)
            })
            .fold(0.0f32, f32::max);

        Self { rows, max_intensity, params: p.clone() }
    }

    /// Render to RGBA pixels at animation time `t`.
    pub fn render(&self, pixels: &mut Vec<egui::Color32>, t: f32) {
        let n = self.params.width;
        let h = self.params.height;
        let inv_max = if self.max_intensity > 0.0 {
            1.0 / self.max_intensity
        } else {
            1.0
        };
        let k = std::f32::consts::TAU / self.params.wavelength;

        pixels.clear();
        pixels.reserve(n * h);

        for (zi, row) in self.rows.iter().enumerate() {
            let z = 1.0 + (zi as f32 / h as f32) * h as f32;
            let z_phase = t - z * k;
            for x in 0..n {
                let re = row.re[x];
                let im = row.im[x];
                let intensity = (re * re + im * im) * inv_max;
                let phase = im.atan2(re) + z_phase;
                let osc = 0.5 + 0.5 * phase.cos();
                let v = intensity.powf(0.45) * osc;
                let r = ((10.0 + 240.0 * (v * 1.5).min(1.0)) as u8).max(0);
                let g = ((30.0 + 200.0 * (v * 1.2).min(1.0)) as u8).max(0);
                let b = ((80.0 + 170.0 * (v * 0.7 + 0.2).min(1.0)) as u8).max(0);
                pixels.push(egui::Color32::from_rgb(r, g, b));
            }
        }
    }
}