use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Alarm {
    pub id: String,
    #[schema(example = "WT001")]
    pub turbine_id: String,
    #[schema(example = "A")]
    pub blade_id: String,
    #[schema(example = "一级")]
    pub alarm_level: String,
    #[schema(example = "delamination_rate")]
    pub alarm_type: String,
    #[schema(example = "分层扩展速率超限：当前6.2 mm/h，阈值5.0 mm/h")]
    pub message: String,
    #[schema(example = 5.0)]
    pub threshold: f64,
    #[schema(example = 6.2)]
    pub actual_value: f64,
    pub timestamp: DateTime<Utc>,
    #[schema(example = 0)]
    pub acknowledged: i32,
    #[schema(example = 0)]
    pub mes_pushed: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlarmLevel {
    Level1,
    Level2,
}

impl std::fmt::Display for AlarmLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlarmLevel::Level1 => write!(f, "一级"),
            AlarmLevel::Level2 => write!(f, "二级"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlarmType {
    DelaminationRate,
    FrequencyOffset,
}

impl std::fmt::Display for AlarmType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlarmType::DelaminationRate => write!(f, "delamination_rate"),
            AlarmType::FrequencyOffset => write!(f, "frequency_offset"),
        }
    }
}
