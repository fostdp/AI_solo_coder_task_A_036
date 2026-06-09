use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{CorsLayer, Any};
use axum::{
    routing::{get, post},
    Router,
};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;

mod models;
mod routes;
mod services;

use services::{
    influxdb_service::InfluxDBService,
    damage_diagnosis::DamageDiagnosisService,
    alarm_engine::AlarmEngine,
    mes_pusher::MesPusher,
};

#[derive(OpenApi)]
#[openapi(
    paths(
        routes::sensor_data::receive_strain_data,
        routes::sensor_data::receive_ae_events,
        routes::sensor_data::receive_damage_features,
        routes::query::get_blade_health,
        routes::query::get_strain_history,
        routes::query::get_ae_events,
        routes::query::get_all_blades_health,
        routes::query::get_damage_statistics,
        routes::query::get_alarms,
        routes::query::get_health_rankings,
        routes::alarm::acknowledge_alarm,
    ),
    components(
        schemas(
            models::strain::StrainData,
            models::ae::AEEvent,
            models::damage::DamageFeatures,
            models::blade::BladeHealth,
            models::alarm::Alarm,
            models::response::ApiResponse,
            models::response::HealthRanking,
            models::response::DamageStatistics,
        )
    ),
    tags(
        (name = "sensor", description = "传感器数据接收接口"),
        (name = "query", description = "数据查询接口"),
        (name = "alarm", description = "告警管理接口"),
    )
)]
struct ApiDoc;

#[derive(Clone)]
pub struct AppState {
    pub influxdb: Arc<InfluxDBService>,
    pub diagnosis: Arc<DamageDiagnosisService>,
    pub alarm_engine: Arc<Mutex<AlarmEngine>>,
    pub mes_pusher: Arc<MesPusher>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    env_logger::init();

    log::info!("启动风力发电机组叶片状态监测系统...");

    let influxdb_service = Arc::new(InfluxDBService::new().await?);
    let diagnosis_service = Arc::new(DamageDiagnosisService::new()?);
    let mes_pusher = Arc::new(MesPusher::new());
    let alarm_engine = Arc::new(Mutex::new(AlarmEngine::new(mes_pusher.clone())));

    let state = AppState {
        influxdb: influxdb_service.clone(),
        diagnosis: diagnosis_service.clone(),
        alarm_engine: alarm_engine.clone(),
        mes_pusher: mes_pusher.clone(),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/health", get(routes::health::health_check))
        .route("/api/v1/sensor/strain", post(routes::sensor_data::receive_strain_data))
        .route("/api/v1/sensor/ae", post(routes::sensor_data::receive_ae_events))
        .route("/api/v1/sensor/damage", post(routes::sensor_data::receive_damage_features))
        .route("/api/v1/blade/health", get(routes::query::get_blade_health))
        .route("/api/v1/blade/all-health", get(routes::query::get_all_blades_health))
        .route("/api/v1/blade/strain-history", get(routes::query::get_strain_history))
        .route("/api/v1/blade/ae-events", get(routes::query::get_ae_events))
        .route("/api/v1/statistics/damage", get(routes::query::get_damage_statistics))
        .route("/api/v1/statistics/health-ranking", get(routes::query::get_health_rankings))
        .route("/api/v1/alarms", get(routes::query::get_alarms))
        .route("/api/v1/alarms/:id/acknowledge", post(routes::alarm::acknowledge_alarm))
        .layer(cors)
        .with_state(state);

    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("{}:{}", host, port);

    log::info!("服务器监听在 {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
