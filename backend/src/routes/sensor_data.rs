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
use crate::services::ethernet_driver::DriverMessage;

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
    log::info!("收到应变数据，共{}条，发送到以太网驱动预处理管道", batch.data.len());

    let influxdb = state.influxdb.clone();
    let data_clone = batch.data.clone();
    tokio::spawn(async move {
        if let Err(e) = influxdb.bulk_write_all(&data_clone, &[], None).await {
            log::error!("批量写入应变数据失败: {}", e);
        }
        for data in &data_clone {
            if let Err(e) = influxdb.update_blade_health_from_strain(data).await {
                log::error!("更新叶片健康状态失败: {}", e);
            }
        }
    });

    match state.pipeline.driver_sender.send(DriverMessage::StrainData(batch.data)).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::success("应变数据已进入预处理管道".to_string())),
        ),
        Err(e) => {
            log::error!("发送应变数据到管道失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<String>::error(&format!("管道发送失败: {}", e))),
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
    log::info!(
        "收到声发射事件，共{}条，发送到以太网驱动预处理管道",
        batch.events.len()
    );

    let influxdb = state.influxdb.clone();
    let events_clone = batch.events.clone();
    tokio::spawn(async move {
        let events_ref: Vec<&AEEvent> = events_clone.iter().collect();
        if let Err(e) = influxdb.write_ae_events(&events_ref).await {
            log::error!("批量写入声发射事件失败: {}", e);
        }
    });

    let mut results = Vec::new();
    for event in batch.events {
        let wind_speed = event.wind_speed.unwrap_or(8.0);
        let rotor_speed = event.rotor_speed.unwrap_or(12.0);
        results.push((event, wind_speed, rotor_speed));
    }

    let mut send_errors = 0;
    for (event, wind, rotor) in results {
        match state.pipeline.driver_sender.send(
            DriverMessage::AEEvent(event, wind, rotor)
        ).await {
            Ok(_) => {}
            Err(e) => {
                log::error!("发送声发射事件到管道失败: {}", e);
                send_errors += 1;
            }
        }
    }

    if send_errors > 0 {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::<String>::error(&format!(
                "{}条事件发送到管道失败", send_errors
            ))),
        )
    } else {
        (
            StatusCode::OK,
            Json(ApiResponse::success(
                "声发射事件已进入预处理管道（小波去噪+工况归一化+随机森林分类异步执行）".to_string()
            )),
        )
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
        "收到损伤特征: {}-{} 损伤严重度: {}，发送到分类器和告警管道",
        features.turbine_id,
        features.blade_id,
        features.damage_severity
    );

    let influxdb = state.influxdb.clone();
    let features_clone = features.clone();
    tokio::spawn(async move {
        if let Err(e) = influxdb.write_damage_features(&features_clone).await {
            log::error!("保存损伤特征失败: {}", e);
        }

        let mut alarm_engine = state.alarm_engine.lock().await;
        if let Err(e) = alarm_engine.check_and_trigger_alarm(&features_clone).await {
            log::error!("告警检查失败: {}", e);
        }
    });

    match state.pipeline.damage_from_driver_sender.send(features).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse::success("损伤特征已进入分类器和告警管道".to_string())),
        ),
        Err(e) => {
            log::error!("发送损伤特征到管道失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<String>::error(&format!("管道发送失败: {}", e))),
            )
        }
    }
}
