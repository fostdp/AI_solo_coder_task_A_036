use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use utoipa::path;

use crate::AppState;
use crate::models::{
    strain::{StrainData, StrainDataBatch},
    ae::{AEEvent, AEEventBatch},
    damage::DamageFeatures,
    response::ApiResponse,
};

#[utoipa::path(
    post,
    path = "/api/v1/sensor/strain",
    tag = "sensor",
    request_body = StrainDataBatch,
    responses(
        (status = 200, description = "应变数据接收成功", body = ApiResponse<String>),
        (status = 400, description = "数据格式错误"),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn receive_strain_data(
    State(state): State<AppState>,
    Json(batch): Json<StrainDataBatch>,
) -> impl IntoResponse {
    log::info!("收到应变数据，共{}条", batch.data.len());

    match state.influxdb.write_strain_data(&batch.data).await {
        Ok(_) => {
            for data in &batch.data {
                if let Err(e) = state.influxdb.update_blade_health_from_strain(data).await {
                    log::error!("更新叶片健康状态失败: {}", e);
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::success("应变数据已保存".to_string())),
            )
        }
        Err(e) => {
            log::error!("保存应变数据失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<String>::error(&format!("保存失败: {}", e))),
            )
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/sensor/ae",
    tag = "sensor",
    request_body = AEEventBatch,
    responses(
        (status = 200, description = "声发射事件接收成功", body = ApiResponse<String>),
        (status = 400, description = "数据格式错误"),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn receive_ae_events(
    State(state): State<AppState>,
    Json(batch): Json<AEEventBatch>,
) -> impl IntoResponse {
    log::info!("收到声发射事件，共{}条", batch.events.len());

    match state.influxdb.write_ae_events(&batch.events).await {
        Ok(_) => {
            for event in &batch.events {
                let diagnosis = state.diagnosis.diagnose(
                    event.amplitude,
                    event.duration,
                    event.frequency_peak,
                    event.frequency_center,
                    event.energy,
                    event.counts,
                );

                let damage_features = DamageFeatures {
                    turbine_id: event.turbine_id.clone(),
                    blade_id: event.blade_id.clone(),
                    section: event.section.clone(),
                    matrix_cracking_prob: diagnosis.matrix_cracking_prob,
                    fiber_breakage_prob: diagnosis.fiber_breakage_prob,
                    delamination_prob: diagnosis.delamination_prob,
                    damage_severity: diagnosis.severity_level as i32 * 25,
                    natural_frequency: 12.5,
                    delamination_rate: diagnosis.delamination_prob * 10.0,
                    health_score: 100 - (diagnosis.severity_level as i32 * 25),
                    timestamp: event.timestamp,
                };

                if let Err(e) = state.influxdb.write_damage_features(&damage_features).await {
                    log::error!("保存损伤特征失败: {}", e);
                }

                let mut alarm_engine = state.alarm_engine.lock().await;
                if let Err(e) = alarm_engine.check_and_trigger_alarm(&damage_features).await {
                    log::error!("告警检查失败: {}", e);
                }
            }

            (
                StatusCode::OK,
                Json(ApiResponse::success("声发射事件已保存并分析完成".to_string())),
            )
        }
        Err(e) => {
            log::error!("保存声发射事件失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<String>::error(&format!("保存失败: {}", e))),
            )
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/sensor/damage",
    tag = "sensor",
    request_body = DamageFeatures,
    responses(
        (status = 200, description = "损伤特征接收成功", body = ApiResponse<String>),
        (status = 400, description = "数据格式错误"),
        (status = 500, description = "服务器内部错误")
    )
)]
pub async fn receive_damage_features(
    State(state): State<AppState>,
    Json(features): Json<DamageFeatures>,
) -> impl IntoResponse {
    log::info!(
        "收到损伤特征: {}-{} 损伤严重度: {}",
        features.turbine_id,
        features.blade_id,
        features.damage_severity
    );

    match state.influxdb.write_damage_features(&features).await {
        Ok(_) => {
            let mut alarm_engine = state.alarm_engine.lock().await;
            if let Err(e) = alarm_engine.check_and_trigger_alarm(&features).await {
                log::error!("告警检查失败: {}", e);
            }

            (
                StatusCode::OK,
                Json(ApiResponse::success("损伤特征已保存".to_string())),
            )
        }
        Err(e) => {
            log::error!("保存损伤特征失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<String>::error(&format!("保存失败: {}", e))),
            )
        }
    }
}
