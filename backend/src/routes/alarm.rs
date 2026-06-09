use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use utoipa::path;

use crate::AppState;
use crate::models::response::ApiResponse;

#[utoipa::path(
    post,
    path = "/api/v1/alarms/{id}/acknowledge",
    tag = "alarm",
    params(
        ("id" = String, Path, description = "告警ID")
    ),
    responses(
        (status = 200, description = "告警确认成功", body = ApiResponse<String>),
        (status = 404, description = "告警不存在"),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn acknowledge_alarm(
    State(state): State<AppState>,
    Path(alarm_id): Path<String>,
) -> impl IntoResponse {
    match state.influxdb.acknowledge_alarm(&alarm_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(ApiResponse::success("告警已确认".to_string())),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::<String>::error("告警不存在")),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<String>::error(&format!("确认失败: {}", e))),
        ),
    }
}
