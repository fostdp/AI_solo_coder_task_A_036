use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct BladeHealth {
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = "A")]
    pub blade_id: String,
    #[schema(example = 85)]
    pub health_score: i32,
    #[schema(example = "none")]
    pub damage_type: String,
    #[schema(example = 0)]
    pub severity_level: u8,
    pub last_check: DateTime<Utc>,
    pub root_health: Option<i32>,
    pub mid_health: Option<i32>,
    pub tip_health: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BladeSectionData {
    pub turbine_id: String,
    pub blade_id: String,
    pub section: String,
    pub strain_history: Vec<(DateTime<Utc>, f64)>,
    pub ae_features: AEMetrics,
    pub damage_prob: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AEMetrics {
    pub total_events: i64,
    pub avg_amplitude: f64,
    pub avg_duration: f64,
    pub avg_frequency: f64,
}
