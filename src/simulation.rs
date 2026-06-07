use rayon::prelude::*;

/// Parameters describing the Mach-Zehnder interferometer geometry.
#[derive(Clone)]
pub struct SimParams {
    pub wavelength: f32,   // λ in sim units
    pub delta: f32,        // extra path length added to arm B (in sim units)
    pub arm_length: f32,   // nominal length of each arm (pixels of propagation to render)
    pub beam_width: f32,   // 1-σ Gaussian half-width of the input beam
    pub width: usize,      // pixel columns
    pub height: usize,     // pixel rows
}

/// A row of complex field amplitudes.
#[derive(Clone)]
pub struct FieldRow {
    pub re: Vec<f32>,
    pub im: Vec<f32>,
}

/// Precomputed field for the full interferometer display.
///
/// Layout (top → bottom in the rendered image):
///   Zone 0   : input beam travelling toward the first beam splitter  (h/5 rows)
///   Zone 1   : arm A (upper path) propagating rightward              (h/5 rows)
///   Zone 2   : arm B (lower path, phase-shifted) propagating         (h/5 rows)
///   Zone 3   : both beams recombining after the second splitter      (h/5 rows)
///   Zone 4   : output / interference fringe region                   (h/5 rows)
pub struct FieldCache {
    pub rows: Vec<FieldRow>,
    pub max_intensity: f32,
    pub params: SimParams,
    /// Which zone each row belongs to (0-4), for rendering hints.
    pub zone: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 1-D Gaussian beam profile centred at `cx` with half-width `sigma`.
fn gaussian_beam(n: usize, cx: f32, sigma: f32) -> (Vec<f32>, Vec<f32>) {
    let re: Vec<f32> = (0..n)
        .map(|x| {
            let d = x as f32 - cx;
            (-d * d / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let im = vec![0.0f32; n];
    (re, im)
}

/// Fresnel propagate a 1-D complex field by distance `z` with wavenumber `k`.
/// Uses the convolution form: U(x, z) = ∫ U(x') h(x-x', z) dx'
/// where h is the Fresnel propagator kernel  ∝  exp(i k (x-x')² / 2z).
fn fresnel_step(re_in: &[f32], im_in: &[f32], z: f32, k: f32) -> (Vec<f32>, Vec<f32>) {
    let n = re_in.len();
    let mut re_out = vec![0.0f32; n];
    let mut im_out = vec![0.0f32; n];

    for x in 0..n {
        let mut sr = 0.0f32;
        let mut si = 0.0f32;
        for xp in 0..n {
            if re_in[xp] == 0.0 && im_in[xp] == 0.0 {
                continue;
            }
            let dx = x as f32 - xp as f32;
            let phase = k * dx * dx / (2.0 * z);
            let (sin_p, cos_p) = phase.sin_cos();
            // (re_in + i·im_in) * (cos - i·sin)  [conjugate kernel for convergence]
            sr += re_in[xp] * cos_p + im_in[xp] * sin_p;
            si += im_in[xp] * cos_p - re_in[xp] * sin_p;
        }
        // normalise by sqrt(z) so intensity doesn't blow up
        let norm = (z + 1.0).sqrt();
        re_out[x] = sr / norm;
        im_out[x] = si / norm;
    }
    (re_out, im_out)
}

/// Apply a uniform phase shift of `phi` radians to every pixel.
fn apply_phase(re: &mut Vec<f32>, im: &mut Vec<f32>, phi: f32) {
    let (s, c) = phi.sin_cos();
    for (r, i) in re.iter_mut().zip(im.iter_mut()) {
        let new_r = *r * c - *i * s;
        let new_i = *r * s + *i * c;
        *r = new_r;
        *i = new_i;
    }
}

/// Pointwise 50/50 beam splitter: transmit and reflect.
/// Returns (transmitted, reflected).
fn beam_split(re: &[f32], im: &[f32]) -> ((Vec<f32>, Vec<f32>), (Vec<f32>, Vec<f32>)) {
    let factor = 1.0_f32 / 2.0_f32.sqrt();
    // transmitted: amplitude × (1/√2)
    let re_t: Vec<f32> = re.iter().map(|v| v * factor).collect();
    let im_t: Vec<f32> = im.iter().map(|v| v * factor).collect();
    // reflected: amplitude × (i/√2)  →  multiply by i ⇒ (re→-im, im→re)
    let re_r: Vec<f32> = im.iter().map(|v| -v * factor).collect();
    let im_r: Vec<f32> = re.iter().map(|v| v * factor).collect();
    ((re_t, im_t), (re_r, im_r))
}

/// Add two complex fields together (recombination).
fn recombine(
    re_a: &[f32], im_a: &[f32],
    re_b: &[f32], im_b: &[f32],
) -> (Vec<f32>, Vec<f32>) {
    let factor = 1.0_f32 / 2.0_f32.sqrt();
    let re: Vec<f32> = re_a.iter().zip(re_b).map(|(a, b)| (a + b) * factor).collect();
    let im: Vec<f32> = im_a.iter().zip(im_b).map(|(a, b)| (a + b) * factor).collect();
    (re, im)
}

// ---------------------------------------------------------------------------
// FieldCache
// ---------------------------------------------------------------------------

impl FieldCache {
    pub fn compute(p: &SimParams) -> Self {
        let n = p.width;
        let h = p.height;
        let k = std::f32::consts::TAU / p.wavelength;

        // Zone heights
        let z0h = h / 5;          // input beam
        let z1h = h / 5;          // arm A
        let z2h = h / 5;          // arm B
        let z3h = h / 5;          // recombination
        let z4h = h - z0h - z1h - z2h - z3h; // output fringes

        // --- Source beam at the first beam splitter ---
        let cx = n as f32 / 2.0;
        let (src_re, src_im) = gaussian_beam(n, cx, p.beam_width);

        // Split into arm A (transmitted) and arm B (reflected, gets extra path)
        let ((mut arm_a_re, mut arm_a_im), (mut arm_b_re, mut arm_b_im)) =
            beam_split(&src_re, &src_im);

        // Arm B picks up the user-controlled extra optical path length
        let extra_phase = k * p.delta;
        apply_phase(&mut arm_b_re, &mut arm_b_im, extra_phase);

        // Propagation step for arms (dz per row inside the arm zone)
        let dz_arm = p.arm_length / z1h.max(1) as f32;

        // --- Zone 0: input beam propagating toward splitter ---
        let z0_rows: Vec<FieldRow> = (0..z0h)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + zi as f32 * dz_arm;
                let (re, im) = fresnel_step(&src_re, &src_im, z, k);
                FieldRow { re, im }
            })
            .collect();

        // --- Zone 1: arm A ---
        // We need sequential snapshots along arm A
        let arm_a_snapshots: Vec<FieldRow> = (0..z1h)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + (zi as f32 + 1.0) * dz_arm;
                let (re, im) = fresnel_step(&arm_a_re, &arm_a_im, z, k);
                FieldRow { re, im }
            })
            .collect();

        // --- Zone 2: arm B ---
        let arm_b_snapshots: Vec<FieldRow> = (0..z2h)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + (zi as f32 + 1.0) * dz_arm;
                let (re, im) = fresnel_step(&arm_b_re, &arm_b_im, z, k);
                FieldRow { re, im }
            })
            .collect();

        // Propagated ends of the two arms (last snapshot)
        let arm_a_end = arm_a_snapshots.last().cloned().unwrap_or(FieldRow {
            re: arm_a_re.clone(),
            im: arm_a_im.clone(),
        });
        let arm_b_end = arm_b_snapshots.last().cloned().unwrap_or(FieldRow {
            re: arm_b_re.clone(),
            im: arm_b_im.clone(),
        });

        // Recombine at the second beam splitter
        let (mut comb_re, mut comb_im) = recombine(
            &arm_a_end.re, &arm_a_end.im,
            &arm_b_end.re, &arm_b_end.im,
        );

        // --- Zone 3: recombination / mixing zone ---
        let dz_out = p.arm_length / z3h.max(1) as f32;
        let z3_rows: Vec<FieldRow> = (0..z3h)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + zi as f32 * dz_out;
                let (re, im) = fresnel_step(&comb_re, &comb_im, z, k);
                FieldRow { re, im }
            })
            .collect();

        // --- Zone 4: output interference pattern ---
        let dz_fringe = p.arm_length * 2.0 / z4h.max(1) as f32;
        let z4_rows: Vec<FieldRow> = (0..z4h)
            .into_par_iter()
            .map(|zi| {
                let z = 1.0 + zi as f32 * dz_fringe;
                let (re, im) = fresnel_step(&comb_re, &comb_im, z, k);
                FieldRow { re, im }
            })
            .collect();

        // Assemble all rows and zone labels
        let mut rows: Vec<FieldRow> = Vec::with_capacity(h);
        let mut zone: Vec<u8> = Vec::with_capacity(h);

        for r in &z0_rows      { rows.push(r.clone()); zone.push(0); }
        for r in &arm_a_snapshots { rows.push(r.clone()); zone.push(1); }
        for r in &arm_b_snapshots { rows.push(r.clone()); zone.push(2); }
        for r in &z3_rows      { rows.push(r.clone()); zone.push(3); }
        for r in &z4_rows      { rows.push(r.clone()); zone.push(4); }

        // Pad if rounding left us short
        while rows.len() < h {
            rows.push(rows.last().cloned().unwrap_or(FieldRow {
                re: vec![0.0; n],
                im: vec![0.0; n],
            }));
            zone.push(4);
        }

        let max_intensity = rows
            .iter()
            .flat_map(|row| {
                row.re.iter().zip(row.im.iter()).map(|(r, i)| r * r + i * i)
            })
            .fold(0.0f32, f32::max);

        Self { rows, max_intensity, params: p.clone(), zone }
    }

    /// Render to RGBA pixels with zone-tinted colouring for clarity.
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
            let z_frac = zi as f32 / h as f32;
            // use a slowly varying z for the travelling-wave phase
            let z_depth = 1.0 + z_frac * self.params.arm_length;
            let z_phase = t - z_depth * k * 0.1;

            let zone = self.zone[zi];

            for x in 0..n {
                let re = row.re[x];
                let im = row.im[x];
                let intensity = (re * re + im * im) * inv_max;
                let phase = im.atan2(re) + z_phase;
                let osc = 0.5 + 0.5 * phase.cos();
                let v = intensity.powf(0.45) * osc;

                // Colour scheme per zone:
                //  0 (input)         : cool white/cyan
                //  1 (arm A upper)   : warm amber
                //  2 (arm B lower)   : cool teal
                //  3 (recombination) : magenta / purple
                //  4 (output)        : bright white-blue (interference fringes)
                let (r, g, b) = match zone {
                    0 => {
                        // Input: cyan-white
                        let r = (20.0  + 200.0 * (v * 1.1).min(1.0)) as u8;
                        let g = (30.0  + 210.0 * (v * 1.1).min(1.0)) as u8;
                        let b = (60.0  + 195.0 * (v * 1.0).min(1.0)) as u8;
                        (r, g, b)
                    }
                    1 => {
                        // Arm A: amber
                        let r = (30.0  + 220.0 * (v * 1.4).min(1.0)) as u8;
                        let g = (20.0  + 160.0 * (v * 1.0).min(1.0)) as u8;
                        let b = (10.0  +  60.0 * (v * 0.5).min(1.0)) as u8;
                        (r, g, b)
                    }
                    2 => {
                        // Arm B: teal
                        let r = (10.0  +  50.0 * (v * 0.5).min(1.0)) as u8;
                        let g = (20.0  + 180.0 * (v * 1.2).min(1.0)) as u8;
                        let b = (40.0  + 210.0 * (v * 1.3).min(1.0)) as u8;
                        (r, g, b)
                    }
                    3 => {
                        // Recombination: magenta
                        let r = (20.0  + 200.0 * (v * 1.4).min(1.0)) as u8;
                        let g = (10.0  +  80.0 * (v * 0.7).min(1.0)) as u8;
                        let b = (30.0  + 200.0 * (v * 1.3).min(1.0)) as u8;
                        (r, g, b)
                    }
                    _ => {
                        // Output: vivid blue-white interference fringes
                        let r = (10.0  + 210.0 * (v * 1.5).min(1.0)) as u8;
                        let g = (20.0  + 220.0 * (v * 1.4).min(1.0)) as u8;
                        let b = (80.0  + 175.0 * (v * 1.0).min(1.0)) as u8;
                        (r, g, b)
                    }
                };

                pixels.push(egui::Color32::from_rgb(r, g, b));
            }
        }
    }
}