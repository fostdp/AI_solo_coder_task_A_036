use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DamageFeatures {
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = "A")]
    pub blade_id: String,
    #[schema(example = "mid")]
    pub section: String,
    #[schema(example = 0.15)]
    pub matrix_cracking_prob: f64,
    #[schema(example = 0.05)]
    pub fiber_breakage_prob: f64,
    #[schema(example = 0.02)]
    pub delamination_prob: f64,
    #[schema(example = 15)]
    pub damage_severity: i32,
    #[schema(example = 12.5)]
    pub natural_frequency: f64,
    #[schema(example = 0.5)]
    pub delamination_rate: f64,
    #[schema(example = 85)]
    pub health_score: i32,
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DamageType {
    None,
    MatrixCracking,
    FiberBreakage,
    Delamination,
    Combined,
}

impl std::fmt::Display for DamageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DamageType::None => write!(f, "none"),
            DamageType::MatrixCracking => write!(f, "matrix"),
            DamageType::FiberBreakage => write!(f, "fiber"),
            DamageType::Delamination => write!(f, "delamination"),
            DamageType::Combined => write!(f, "combined"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResult {
    pub damage_type: DamageType,
    pub severity_level: u8,
    pub confidence: f64,
    pub matrix_cracking_prob: f64,
    pub fiber_breakage_prob: f64,
    pub delamination_prob: f64,
}
