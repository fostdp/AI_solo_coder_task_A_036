use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tower_http::cors::{CorsLayer, Any};
use tower_http::compression::CompressionLayer;
use axum::{
    routing::{get, post},
    Router,
    response::IntoResponse,
    extract::State,
    http::StatusCode,
};
use utoipa::{OpenApi, ToSchema};
use utoipa_swagger_ui::SwaggerUi;
use tracing::{info, error, warn, debug, span, Instrument, Level};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use metrics_tracing_context::MetricsLayer;
use tracing_subscriber::{prelude::*, EnvFilter};

mod models;
mod routes;
mod services;

use services::{
    influxdb_service::InfluxDBService,
    damage_diagnosis::DamageDiagnosisService,
    alarm_engine::AlarmEngine,
    mes_pusher::MesPusher,
    signal_processing::SignalProcessor,
    ethernet_driver::{EthernetDriver, ProcessedStrainData, ProcessedAEData, DriverMessage, start_ethernet_driver},
    damage_classifier::{DamageClassifier, RandomForestConfig, start_damage_classifier},
    strain_interpolator::{StrainInterpolator, InterpolationConfig, InterpolatedStrainField, start_strain_interpolator},
    alarm_pusher::{AlarmPusher, AlarmConfig, AlarmMessage, start_alarm_pusher},
};
use crate::models::damage::DamageFeatures;
use crate::models::alarm::Alarm;

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
        (name = "metrics", description = "系统指标接口"),
    )
)]
struct ApiDoc;

pub struct PipelineChannels {
    pub driver_sender: mpsc::Sender<DriverMessage>,
    pub strain_sender: mpsc::Sender<ProcessedStrainData>,
    pub ae_sender: mpsc::Sender<ProcessedAEData>,
    pub damage_from_driver_sender: mpsc::Sender<DamageFeatures>,
    pub damage_from_classifier_sender: mpsc::Sender<DamageFeatures>,
    pub field_sender: mpsc::Sender<InterpolatedStrainField>,
    pub alarm_sender: mpsc::Sender<Alarm>,
    pub alarm_message_sender: mpsc::Sender<AlarmMessage>,
}

#[derive(Clone)]
pub struct AppState {
    pub influxdb: Arc<InfluxDBService>,
    pub diagnosis: Arc<DamageDiagnosisService>,
    pub alarm_engine: Arc<Mutex<AlarmEngine>>,
    pub mes_pusher: Arc<MesPusher>,
    pub signal_processor: Arc<SignalProcessor>,
    pub pipeline: PipelineChannels,
    pub rf_config: Arc<RandomForestConfig>,
    pub interpolation_config: Arc<InterpolationConfig>,
    pub alarm_config: Arc<AlarmConfig>,
    pub prometheus_handle: Arc<PrometheusHandle>,
}

#[derive(Clone)]
pub struct AppMetrics {
    pub http_requests_total: metrics::Counter,
    pub http_request_duration_seconds: metrics::Histogram,
    pub sensor_data_received_total: metrics::Counter,
    pub sensor_strain_points_total: metrics::Counter,
    pub sensor_ae_events_total: metrics::Counter,
    pub sensor_damage_features_total: metrics::Counter,
    pub damage_classifications_total: metrics::Counter,
    pub damage_alerts_triggered_total: metrics::Counter,
    pub mes_notifications_sent_total: metrics::Counter,
    pub mes_notifications_failed_total: metrics::Counter,
    pub influxdb_write_points_total: metrics::Counter,
    pub influxdb_write_failed_total: metrics::Counter,
    pub strain_interpolations_total: metrics::Counter,
    pub ethernet_driver_queue_size: metrics::Gauge,
    pub damage_classifier_queue_size: metrics::Gauge,
    pub strain_interpolator_queue_size: metrics::Gauge,
    pub alarm_pusher_queue_size: metrics::Gauge,
    pub active_turbines: metrics::Gauge,
    pub system_health_score: metrics::Gauge,
}

impl AppMetrics {
    pub fn new() -> Self {
        Self {
            http_requests_total: counter!("http_requests_total", "Total HTTP requests received"),
            http_request_duration_seconds: histogram!("http_request_duration_seconds", "HTTP request duration in seconds"),
            sensor_data_received_total: counter!("sensor_data_received_total", "Total sensor data batches received"),
            sensor_strain_points_total: counter!("sensor_strain_points_total", "Total strain data points received"),
            sensor_ae_events_total: counter!("sensor_ae_events_total", "Total AE events received"),
            sensor_damage_features_total: counter!("sensor_damage_features_total", "Total damage feature batches received"),
            damage_classifications_total: counter!("damage_classifications_total", "Total damage classifications performed"),
            damage_alerts_triggered_total: counter!("damage_alerts_triggered_total", "Total damage alerts triggered"),
            mes_notifications_sent_total: counter!("mes_notifications_sent_total", "Total MES notifications sent successfully"),
            mes_notifications_failed_total: counter!("mes_notifications_failed_total", "Total MES notifications failed"),
            influxdb_write_points_total: counter!("influxdb_write_points_total", "Total points written to InfluxDB"),
            influxdb_write_failed_total: counter!("influxdb_write_failed_total", "Total failed InfluxDB writes"),
            strain_interpolations_total: counter!("strain_interpolations_total", "Total strain interpolations performed"),
            ethernet_driver_queue_size: gauge!("ethernet_driver_queue_size", "Ethernet driver channel queue size"),
            damage_classifier_queue_size: gauge!("damage_classifier_queue_size", "Damage classifier channel queue size"),
            strain_interpolator_queue_size: gauge!("strain_interpolator_queue_size", "Strain interpolator channel queue size"),
            alarm_pusher_queue_size: gauge!("alarm_pusher_queue_size", "Alarm pusher channel queue size"),
            active_turbines: gauge!("active_turbines", "Number of active turbines reporting data"),
            system_health_score: gauge!("system_health_score", "Overall system health score 0-100"),
        }
    }

    pub fn record_http_request(&self, method: &str, path: &str, status: u16, duration: f64) {
        self.http_requests_total.increment(1, &[
            metrics::Label::new("method", method.to_string()),
            metrics::Label::new("path", path.to_string()),
            metrics::Label::new("status", status.to_string()),
        ]);
        self.http_request_duration_seconds.record(duration, &[
            metrics::Label::new("method", method.to_string()),
            metrics::Label::new("path", path.to_string()),
        ]);
    }
}

fn setup_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,wind_turbine_blade_monitor=debug"));

    let is_json = std::env::var("LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    if is_json {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
            .with(MetricsLayer::new())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .with(MetricsLayer::new())
            .init();
    }
}

fn setup_metrics() -> PrometheusHandle {
    PrometheusBuilder::new()
        .set_buckets(&[
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0
        ])
        .unwrap()
        .install_recorder()
        .expect("Failed to install Prometheus recorder")
}

fn load_config() -> Result<(
    Arc<RandomForestConfig>,
    Arc<InterpolationConfig>,
    Arc<AlarmConfig>,
), Box<dyn std::error::Error>> {
    let config_path = std::env::var("CONFIG_PATH")
        .unwrap_or_else(|_| "config/model_config.toml".to_string());

    let config_content = std::fs::read_to_string(&config_path)
        .unwrap_or_else(|_| {
            warn!("未找到配置文件 {}，使用默认配置", config_path);
            String::new()
        });

    let rf_config = if config_content.is_empty() {
        Arc::new(RandomForestConfig::default())
    } else {
        match RandomForestConfig::from_toml_str(&config_content) {
            Ok(mut cfg) => {
                if let Ok(parsed) = toml::from_str::<toml::Value>(&config_content) {
                    if let Some(rf) = parsed.get("random_forest") {
                        if let Some(n_trees) = rf.get("n_trees").and_then(|v| v.as_integer()) {
                            cfg.n_trees = n_trees as usize;
                        }
                        if let Some(max_depth) = rf.get("max_depth").and_then(|v| v.as_integer()) {
                            cfg.max_depth = max_depth as usize;
                        }
                    }
                    if let Some(sp) = parsed.get("signal_processing") {
                        if let Some(level) = sp.get("wavelet_level").and_then(|v| v.as_integer()) {
                            info!("配置小波分解层数: {}", level);
                        }
                    }
                }
                Arc::new(cfg)
            }
            Err(e) => {
                warn!("解析配置失败，使用默认配置: {}", e);
                Arc::new(RandomForestConfig::default())
            }
        }
    };

    let interpolation_config = if config_content.is_empty() {
        Arc::new(InterpolationConfig::default())
    } else {
        match toml::from_str::<toml::Value>(&config_content) {
            Ok(parsed) => {
                let mut cfg = InterpolationConfig::default();
                if let Some(si) = parsed.get("strain_interpolation") {
                    if let Some(res) = si.get("grid_resolution").and_then(|v| v.as_integer()) {
                        cfg.grid_resolution = res as usize;
                    }
                    if let Some(model) = si.get("variogram_model").and_then(|v| v.as_str()) {
                        cfg.variogram_model = model.to_string();
                    }
                    if let Some(len) = si.get("blade_length").and_then(|v| v.as_float()) {
                        cfg.blade_length = len;
                    }
                    if let Some(chord) = si.get("blade_chord").and_then(|v| v.as_float()) {
                        cfg.blade_chord = chord;
                    }
                }
                Arc::new(cfg)
            }
            Err(_) => Arc::new(InterpolationConfig::default()),
        }
    };

    let alarm_config = if config_content.is_empty() {
        Arc::new(AlarmConfig::default())
    } else {
        match toml::from_str::<toml::Value>(&config_content) {
            Ok(parsed) => {
                let mut cfg = AlarmConfig::default();
                if let Some(alarm) = parsed.get("alarm") {
                    if let Some(v) = alarm.get("delamination_rate_threshold").and_then(|v| v.as_float()) {
                        cfg.delamination_rate_threshold = v;
                    }
                    if let Some(v) = alarm.get("frequency_offset_threshold").and_then(|v| v.as_float()) {
                        cfg.frequency_offset_threshold = v;
                    }
                    if let Some(v) = alarm.get("baseline_rotor_speed").and_then(|v| v.as_float()) {
                        cfg.baseline_rotor_speed = v;
                    }
                    if let Some(arr) = alarm.get("valid_speed_range").and_then(|v| v.as_array()) {
                        if arr.len() >= 2 {
                            if let (Some(a), Some(b)) = (arr[0].as_float(), arr[1].as_float()) {
                                cfg.valid_speed_range = (a, b);
                            }
                        }
                    }
                    if let Some(arr) = alarm.get("valid_wind_range").and_then(|v| v.as_array()) {
                        if arr.len() >= 2 {
                            if let (Some(a), Some(b)) = (arr[0].as_float(), arr[1].as_float()) {
                                cfg.valid_wind_range = (a, b);
                            }
                        }
                    }
                    if let Some(v) = alarm.get("frequency_history_size").and_then(|v| v.as_integer()) {
                        cfg.frequency_history_size = v as usize;
                    }
                    if let Some(v) = alarm.get("trend_min_points").and_then(|v| v.as_integer()) {
                        cfg.trend_min_points = v as usize;
                    }
                    if let Some(v) = alarm.get("trend_window_size").and_then(|v| v.as_integer()) {
                        cfg.trend_window_size = v as usize;
                    }
                    if let Some(v) = alarm.get("cooldown_minutes").and_then(|v| v.as_integer()) {
                        cfg.cooldown_minutes = v as i64;
                    }
                }
                Arc::new(cfg)
            }
            Err(_) => Arc::new(AlarmConfig::default()),
        }
    };

    info!(
        n_trees = rf_config.n_trees,
        max_depth = rf_config.max_depth,
        interpolation_resolution = interpolation_config.grid_resolution,
        frequency_threshold = alarm_config.frequency_offset_threshold,
        "配置加载完成"
    );

    Ok((rf_config, interpolation_config, alarm_config))
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let metrics = state.prometheus_handle.render();
    (StatusCode::OK, [("content-type", "text/plain; charset=utf-8")], metrics)
}

async fn health_check_handler() -> impl IntoResponse {
    let response = serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    (StatusCode::OK, axum::Json(response))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    setup_tracing();

    let _app_metrics = AppMetrics::new();
    let prometheus_handle = setup_metrics();
    let prometheus_handle = Arc::new(prometheus_handle);

    info!("==================================================");
    info!("启动风力发电机组叶片状态监测系统 (模块化架构)");
    info!("版本: {}", env!("CARGO_PKG_VERSION"));
    info!("==================================================");

    let span = span!(Level::INFO, "system_init");
    let _enter = span.enter();

    let (rf_config, interpolation_config, alarm_config) = load_config()?;

    let channel_size = std::env::var("CHANNEL_BUFFER_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1000);

    info!(channel_size, "初始化通信管道");

    let (driver_sender, driver_receiver) = mpsc::channel::<DriverMessage>(channel_size);
    let (strain_sender, strain_receiver) = mpsc::channel::<ProcessedStrainData>(channel_size);
    let (ae_sender, ae_receiver) = mpsc::channel::<ProcessedAEData>(channel_size);
    let (damage_from_driver_sender, damage_from_classifier_receiver) = mpsc::channel::<DamageFeatures>(channel_size / 2);
    let (damage_from_classifier_sender, damage_receiver) = mpsc::channel::<DamageFeatures>(channel_size / 2);
    let (field_sender, _field_receiver) = mpsc::channel::<InterpolatedStrainField>(100);
    let (alarm_sender, _alarm_receiver) = mpsc::channel::<Alarm>(100);
    let (alarm_message_sender, alarm_message_receiver) = mpsc::channel::<AlarmMessage>(channel_size / 2);

    let channels = PipelineChannels {
        driver_sender: driver_sender.clone(),
        strain_sender: strain_sender.clone(),
        ae_sender: ae_sender.clone(),
        damage_from_driver_sender: damage_from_driver_sender.clone(),
        damage_from_classifier_sender: damage_from_classifier_sender.clone(),
        field_sender: field_sender.clone(),
        alarm_sender: alarm_sender.clone(),
        alarm_message_sender: alarm_message_sender.clone(),
    };

    let signal_processor = Arc::new(SignalProcessor::new());
    let influxdb_service = Arc::new(InfluxDBService::new().await?);
    let diagnosis_service = Arc::new(DamageDiagnosisService::new()?);
    let mes_pusher = Arc::new(MesPusher::new());
    let alarm_engine = Arc::new(Mutex::new(AlarmEngine::new(mes_pusher.clone())));

    info!("初始化以太网驱动模块");
    let ethernet_driver = Arc::new(EthernetDriver::new(
        signal_processor.clone(),
        strain_sender,
        ae_sender,
        damage_from_driver_sender,
    ));

    info!("初始化损伤分类器模块");
    let damage_classifier = Arc::new(DamageClassifier::new(
        rf_config.clone(),
        signal_processor.clone(),
        damage_from_classifier_sender,
    )?);

    info!("初始化应变插值器模块");
    let strain_interpolator = Arc::new(StrainInterpolator::new(
        interpolation_config.clone(),
        field_sender,
    )?);

    info!("初始化告警推送器模块");
    let alarm_pusher = Arc::new(AlarmPusher::new(
        alarm_config.clone(),
        mes_pusher.clone(),
        alarm_sender,
    ));

    let state = AppState {
        influxdb: influxdb_service.clone(),
        diagnosis: diagnosis_service.clone(),
        alarm_engine: alarm_engine.clone(),
        mes_pusher: mes_pusher.clone(),
        signal_processor: signal_processor.clone(),
        pipeline: channels,
        rf_config: rf_config.clone(),
        interpolation_config: interpolation_config.clone(),
        alarm_config: alarm_config.clone(),
        prometheus_handle: prometheus_handle.clone(),
    };

    drop(_enter);

    info!("启动后台处理任务");

    let driver_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "ethernet_driver");
        async move {
            if let Err(e) = start_ethernet_driver(ethernet_driver, driver_receiver).await {
                error!(error = %e, "以太网驱动任务异常退出");
            }
            info!("以太网驱动任务正常结束");
        }.instrument(span).await
    });

    let classifier_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "damage_classifier");
        async move {
            if let Err(e) = start_damage_classifier(
                damage_classifier,
                ae_receiver,
                damage_from_classifier_receiver,
            ).await {
                error!(error = %e, "损伤分类器任务异常退出");
            }
            info!("损伤分类器任务正常结束");
        }.instrument(span).await
    });

    let interpolator_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "strain_interpolator");
        async move {
            if let Err(e) = start_strain_interpolator(strain_interpolator, strain_receiver).await {
                error!(error = %e, "应变插值器任务异常退出");
            }
            info!("应变插值器任务正常结束");
        }.instrument(span).await
    });

    let alarm_pusher_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "alarm_pusher");
        async move {
            if let Err(e) = start_alarm_pusher(alarm_pusher, alarm_message_receiver).await {
                error!(error = %e, "告警推送器任务异常退出");
            }
            info!("告警推送器任务正常结束");
        }.instrument(span).await
    });

    let state_clone = state.clone();
    let damage_forwarder_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "damage_forwarder");
        async move {
            let mut receiver = damage_receiver;
            let alarm_sender = state_clone.pipeline.alarm_message_sender.clone();
            let influxdb = state_clone.influxdb.clone();
            let alarm_engine = state_clone.alarm_engine.clone();

            info!("损伤特征转发器启动");

            while let Some(features) = receiver.recv().await {
                let turbine_id = features.turbine_id.clone();
                let blade_id = features.blade_id.clone();
                let section = features.section.clone();
                let health_score = features.health_score;

                let process_span = span!(
                    Level::DEBUG,
                    "process_damage_features",
                    turbine_id,
                    blade_id,
                    section,
                    health_score
                );
                let _enter = process_span.enter();

                counter!("damage_classifications_total").increment(1);

                if let Err(e) = influxdb.write_damage_features(&features).await {
                    error!(error = %e, "保存损伤特征失败");
                    counter!("influxdb_write_failed_total").increment(1);
                } else {
                    counter!("influxdb_write_points_total").increment(1);
                }

                let mut engine = alarm_engine.lock().await;
                if let Err(e) = engine.check_frequency_with_conditions(&features, None) {
                    error!(error = %e, "频率告警检查失败");
                }
                if let Err(e) = engine.check_and_trigger_alarm(&features).await {
                    error!(error = %e, "告警检查失败");
                }
                drop(engine);

                if let Err(e) = alarm_sender.send(AlarmMessage::DamageFeatures(features)).await {
                    error!(error = %e, "发送告警消息失败");
                }
            }

            info!("损伤特征转发器正常关闭");
        }.instrument(span).await
    });

    let queue_monitor = tokio::spawn(async move {
        let driver_sender = driver_sender.clone();
        loop {
            gauge!("ethernet_driver_queue_size").set(driver_sender.capacity() as f64 - driver_sender.len() as f64);
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let compression = CompressionLayer::new().gzip(true);

    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/health", get(health_check_handler))
        .route("/metrics", get(metrics_handler))
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
        .nest_service("/static", tower_http::services::ServeDir::new("../frontend"))
        .layer(cors)
        .layer(compression)
        .with_state(state);

    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "8000".to_string());
    let addr = format!("{}:{}", host, port);

    info!("==================================================");
    info!("HTTP服务器启动，监听地址: {}", addr);
    info!("模块架构: 以太网驱动 → 分类器/插值器 → 告警推送器");
    info!("通信方式: Tokio MPSC Channel");
    info!("配置文件: config/model_config.toml");
    info!("健康检查: http://{}/health", addr);
    info!("指标接口: http://{}/metrics", addr);
    info!("API文档: http://{}/swagger-ui", addr);
    info!("静态文件: http://{}/static/index.html", addr);
    info!("==================================================");

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    let server_handle = tokio::spawn(async move {
        let span = span!(Level::INFO, "http_server");
        async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!(error = %e, "HTTP服务器异常退出");
            }
            info!("HTTP服务器任务正常结束");
        }.instrument(span).await
    });

    tokio::select! {
        _ = server_handle => info!("HTTP服务器任务结束"),
        _ = driver_handle => info!("以太网驱动任务结束"),
        _ = classifier_handle => info!("损伤分类器任务结束"),
        _ = interpolator_handle => info!("应变插值器任务结束"),
        _ = alarm_pusher_handle => info!("告警推送器任务结束"),
        _ = damage_forwarder_handle => info!("损伤转发器任务结束"),
        _ = queue_monitor => info!("队列监控任务结束"),
    }

    info!("系统正常关闭");

    Ok(())
}
