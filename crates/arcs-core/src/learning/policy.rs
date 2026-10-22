/// Konservative Parameter für lokale Gewichtsverstärkung.
#[derive(Clone, Debug)]
pub struct LearningPolicy {
    pub success_increment: f64,
    pub min_weight: f64,
    pub max_weight: f64,
}

impl Default for LearningPolicy {
    fn default() -> Self {
        Self {
            success_increment: 0.05,
            min_weight: -1.0,
            max_weight: 1.0,
        }
    }
}
