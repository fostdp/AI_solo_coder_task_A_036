use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::collections::HashMap;
use utoipa::path;

use crate::AppState;
use crate::models::{
    blade::BladeHealth,
    response::{ApiResponse, DamageStatistics, HealthRanking, StrainHistoryPoint, AEEventPoint},
    alarm::Alarm,
};

#[derive(Debug, serde::Deserialize)]
pub struct BladeQuery {
    pub turbine_id: String,
    pub blade_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct HistoryQuery {
    pub turbine_id: String,
    pub blade_id: String,
    pub section: Option<String>,
    pub hours: Option<i64>,
}

#[utoipa::path(
    get,
    path = "/api/v1/blade/health",
    tag = "query",
    params(
        ("turbine_id" = String, Query, description = "风机编号"),
        ("blade_id" = String, Query, description = "叶片编号")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<BladeHealth>),
        (status = 404, description = "未找到数据"),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_blade_health(
    State(state): State<AppState>,
    Query(params): Query<BladeQuery>,
) -> impl IntoResponse {
    match state
        .influxdb
        .get_blade_health(&params.turbine_id, &params.blade_id)
        .await
    {
        Ok(Some(health)) => (StatusCode::OK, Json(ApiResponse::success(health))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<BladeHealth>::error("未找到叶片健康数据")),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<BladeHealth>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/blade/all-health",
    tag = "query",
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<Vec<BladeHealth>>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_all_blades_health(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.influxdb.get_all_blades_health().await {
        Ok(health_list) => (StatusCode::OK, Json(ApiResponse::success(health_list))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<Vec<BladeHealth>>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/blade/strain-history",
    tag = "query",
    params(
        ("turbine_id" = String, Query, description = "风机编号"),
        ("blade_id" = String, Query, description = "叶片编号"),
        ("section" = Option<String>, Query, description = "叶片截面"),
        ("hours" = Option<i64>, Query, description = "查询小时数")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<Vec<StrainHistoryPoint>>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_strain_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let hours = params.hours.unwrap_or(24);
    let section = params.section.as_deref().unwrap_or("mid");

    match state
        .influxdb
        .get_strain_history(&params.turbine_id, &params.blade_id, section, hours)
        .await
    {
        Ok(history) => {
            let points: Vec<StrainHistoryPoint> = history
                .into_iter()
                .map(|(time, value)| StrainHistoryPoint { time, value })
                .collect();
            (StatusCode::OK, Json(ApiResponse::success(points)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<Vec<StrainHistoryPoint>>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/blade/ae-events",
    tag = "query",
    params(
        ("turbine_id" = String, Query, description = "风机编号"),
        ("blade_id" = String, Query, description = "叶片编号"),
        ("section" = Option<String>, Query, description = "叶片截面"),
        ("hours" = Option<i64>, Query, description = "查询小时数")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<Vec<AEEventPoint>>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_ae_events(
    State(state): State<AppState>,
    Query(params): Query<HistoryQuery>,
) -> impl IntoResponse {
    let hours = params.hours.unwrap_or(24);
    let section = params.section.as_deref().unwrap_or("mid");

    match state
        .influxdb
        .get_ae_events(&params.turbine_id, &params.blade_id, section, hours)
        .await
    {
        Ok(events) => {
            let points: Vec<AEEventPoint> = events
                .into_iter()
                .map(|(time, amp, dur, freq)| AEEventPoint {
                    time,
                    amplitude: amp,
                    duration: dur,
                    frequency: freq,
                })
                .collect();
            (StatusCode::OK, Json(ApiResponse::success(points)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<Vec<AEEventPoint>>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/statistics/damage",
    tag = "query",
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<DamageStatistics>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_damage_statistics(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.influxdb.get_all_blades_health().await {
        Ok(health_list) => {
            let total_blades = health_list.len() as i32;
            let mut stats = DamageStatistics {
                total_blades,
                healthy_count: 0,
                warning_count: 0,
                damaged_count: 0,
                matrix_cracking_count: 0,
                fiber_breakage_count: 0,
                delamination_count: 0,
                avg_health_score: 0.0,
                active_alarms: 0,
            };

            let mut total_score: i64 = 0;

            for health in &health_list {
                total_score += health.health_score as i64;

                match health.health_score {
                    80..=100 => stats.healthy_count += 1,
                    60..=79 => stats.warning_count += 1,
                    _ => stats.damaged_count += 1,
                }

                match health.damage_type.as_str() {
                    "matrix" => stats.matrix_cracking_count += 1,
                    "fiber" => stats.fiber_breakage_count += 1,
                    "delamination" => stats.delamination_count += 1,
                    _ => {}
                }
            }

            if total_blades > 0 {
                stats.avg_health_score = total_score as f64 / total_blades as f64;
            }

            match state.influxdb.get_active_alarms_count().await {
                Ok(count) => stats.active_alarms = count,
                Err(e) => log::warn!("获取活跃告警数失败: {}", e),
            }

            (StatusCode::OK, Json(ApiResponse::success(stats)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<DamageStatistics>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/statistics/health-ranking",
    tag = "query",
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<Vec<HealthRanking>>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_health_rankings(
    State(state): State<AppState>,
) -> impl IntoResponse {
    match state.influxdb.get_all_blades_health().await {
        Ok(health_list) => {
            let mut turbine_scores: HashMap<String, i32> = HashMap::new();

            for health in &health_list {
                let entry = turbine_scores.entry(health.turbine_id.clone()).or_insert(0);
                *entry += health.health_score;
            }

            let mut rankings: Vec<HealthRanking> = turbine_scores
                .into_iter()
                .map(|(turbine_id, total_score)| HealthRanking {
                    turbine_id: turbine_id.clone(),
                    health_score: total_score / 3,
                    rank: 0,
                })
                .collect();

            rankings.sort_by(|a, b| b.health_score.cmp(&a.health_score));

            for (i, r) in rankings.iter_mut().enumerate() {
                r.rank = (i + 1) as i32;
            }

            (StatusCode::OK, Json(ApiResponse::success(rankings)))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<Vec<HealthRanking>>::error(&format!("查询失败: {}", e))),
        ),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/alarms",
    tag = "query",
    params(
        ("limit" = Option<i64>, Query, description = "返回数量限制"),
        ("acknowledged" = Option<i32>, Query, description = "是否已确认")
    ),
    responses(
        (status = 200, description = "查询成功", body = ApiResponse<Vec<Alarm>>),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn get_alarms(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(100);
    let acknowledged = params
        .get("acknowledged")
        .and_then(|v| v.parse::<i32>().ok());

    match state.influxdb.get_alarms(limit, acknowledged).await {
        Ok(alarms) => (StatusCode::OK, Json(ApiResponse::success(alarms))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<Vec<Alarm>>::error(&format!("查询失败: {}", e))),
        ),
    }
}
