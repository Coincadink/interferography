// simulation.rs — sine wave parameters only

#[derive(Clone)]
pub struct WaveParams {
    pub amplitude: f32,   // 0.1 – 1.0
    pub frequency: f32,   // 0.5 – 10.0  (cycles across the view)
    pub phase:     f32,   // 0.0 – 6.28  (radians)
    pub speed:     f32,   // 0.0 – 5.0   (animation speed)
}

impl Default for WaveParams {
    fn default() -> Self {
        Self {
            amplitude: 0.5,
            frequency: 2.0,
            phase:     0.0,
            speed:     1.0,
        }
    }
}