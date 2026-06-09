use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AEEvent {
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = "A")]
    pub blade_id: String,
    #[schema(example = "AE01")]
    pub sensor_id: String,
    #[schema(example = "mid")]
    pub section: String,
    #[schema(example = 95.5)]
    pub amplitude: f64,
    #[schema(example = 1500.0)]
    pub duration: f64,
    #[schema(example = 250.0)]
    pub frequency_peak: f64,
    #[schema(example = 180.0)]
    pub frequency_center: f64,
    #[schema(example = 12500.0)]
    pub energy: f64,
    #[schema(example = 45)]
    pub counts: i32,
    #[schema(example = 250.0)]
    pub rise_time: f64,
    #[schema(example = 8.5)]
    pub wind_speed: Option<f64>,
    #[schema(example = 12.0)]
    pub rotor_speed: Option<f64>,
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AEEventBatch {
    pub events: Vec<AEEvent>,
}
