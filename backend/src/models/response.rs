use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use chrono::DateTime;
use chrono::Utc;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            message: "操作成功".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: &str) -> Self {
        Self {
            success: false,
            message: message.to_string(),
            data: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthRanking {
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = 95)]
    pub health_score: i32,
    #[schema(example = 1)]
    pub rank: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct DamageStatistics {
    pub total_blades: i32,
    pub healthy_count: i32,
    pub warning_count: i32,
    pub damaged_count: i32,
    pub matrix_cracking_count: i32,
    pub fiber_breakage_count: i32,
    pub delamination_count: i32,
    pub avg_health_score: f64,
    pub active_alarms: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrainHistoryPoint {
    pub time: DateTime<Utc>,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AEEventPoint {
    pub time: DateTime<Utc>,
    pub amplitude: f64,
    pub duration: f64,
    pub frequency: f64,
}
