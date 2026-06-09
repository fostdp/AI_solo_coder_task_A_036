use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StrainData {
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = "A")]
    pub blade_id: String,
    #[schema(example = "S01")]
    pub sensor_id: String,
    #[schema(example = "root")]
    pub section: String,
    #[schema(example = 1250.5)]
    pub strain_value: f64,
    #[schema(example = 25.3)]
    pub temperature: f64,
    #[schema(example = 0.5)]
    pub position_x: f64,
    #[schema(example = 0.2)]
    pub position_y: f64,
    #[schema(example = 10.5)]
    pub position_z: f64,
    #[schema(example = 8.5)]
    pub wind_speed: Option<f64>,
    #[schema(example = 12.0)]
    pub rotor_speed: Option<f64>,
    #[serde(default = "Utc::now")]
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StrainDataBatch {
    pub data: Vec<StrainData>,
}
