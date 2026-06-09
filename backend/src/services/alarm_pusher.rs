use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use thiserror::Error;
use serde::Deserialize;
use chrono::{DateTime, Utc, Duration};

use crate::models::damage::DamageFeatures;
use crate::models::alarm::{Alarm, AlarmLevel, AlarmType};
use crate::services::mes_pusher::MesPusher;
use crate::services::signal_processing::{OrderTracker, SignalProcessingError};

#[derive(Error, Debug)]
pub enum AlarmPusherError {
    #[error("信号处理失败: {0}")]
    SignalProcessingError(#[from] SignalProcessingError),
    #[error("MES推送失败: {0}")]
    MesPushError(String),
    #[error("通道发送失败: {0}")]
    ChannelSendError(String),
    #[error("配置错误: {0}")]
    ConfigError(String),
    #[error("告警创建失败: {0}")]
    AlarmCreationError(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct AlarmConfig {
    pub delamination_rate_threshold: f64,
    pub frequency_offset_threshold: f64,
    pub baseline_rotor_speed: f64,
    pub valid_speed_range: (f64, f64),
    pub valid_wind_range: (f64, f64),
    pub frequency_history_size: usize,
    pub trend_min_points: usize,
    pub trend_window_size: usize,
    pub cooldown_minutes: i64,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self {
            delamination_rate_threshold: 5.0,
            frequency_offset_threshold: 10.0,
            baseline_rotor_speed: 12.0,
            valid_speed_range: (9.0, 15.0),
            valid_wind_range: (3.0, 25.0),
            frequency_history_size: 10,
            trend_min_points: 3,
            trend_window_size: 5,
            cooldown_minutes: 30,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AlarmMessage {
    DamageFeatures(DamageFeatures),
    VibrationSignal {
        features: DamageFeatures,
        signal: Vec<f64>,
    },
    Acknowledge(String),
    Shutdown,
}

pub struct AlarmPusher {
    config: Arc<AlarmConfig>,
    mes_pusher: Arc<MesPusher>,
    order_tracker: OrderTracker,
    alarm_sender: mpsc::Sender<Alarm>,
    cooldown_periods: Arc<Mutex<std::collections::HashMap<String, DateTime<Utc>>>>,
    frequency_history: Arc<Mutex<std::collections::HashMap<String, Vec<(DateTime<Utc>, f64)>>>>,
    baseline_frequency: f64,
}

impl AlarmPusher {
    pub fn new(
        config: Arc<AlarmConfig>,
        mes_pusher: Arc<MesPusher>,
        alarm_sender: mpsc::Sender<Alarm>,
    ) -> Self {
        Self {
            config: config.clone(),
            mes_pusher,
            order_tracker: OrderTracker::new(1000.0, 10.0, 0.1),
            alarm_sender,
            cooldown_periods: Arc::new(Mutex::new(std::collections::HashMap::new())),
            frequency_history: Arc::new(Mutex::new(std::collections::HashMap::new())),
            baseline_frequency: 12.5,
        }
    }

    pub fn with_baseline_frequency(mut self, freq: f64) -> Self {
        self.baseline_frequency = freq;
        self
    }

    pub async fn process_damage_features(
        &self,
        features: DamageFeatures,
    ) -> Result<(), AlarmPusherError> {
        log::debug!(
            "告警推送器处理损伤特征: {}-{} {} 健康度={}",
            features.turbine_id,
            features.blade_id,
            features.section,
            features.health_score
        );

        self.check_delamination_rate(&features).await?;
        self.check_frequency_offset(&features, None).await?;

        Ok(())
    }

    pub async fn process_vibration_signal(
        &self,
        features: &DamageFeatures,
        signal: &[f64],
    ) -> Result<(), AlarmPusherError> {
        self.check_frequency_offset(features, Some(signal)).await?;
        Ok(())
    }

    async fn check_delamination_rate(
        &self,
        features: &DamageFeatures,
    ) -> Result<(), AlarmPusherError> {
        if features.delamination_rate < self.config.delamination_rate_threshold {
            return Ok(());
        }

        let key = format!(
            "delam_{}_{}_{}",
            features.turbine_id, features.blade_id, features.section
        );

        if !self.is_in_cooldown(&key).await {
            let alarm = self.create_alarm(
                features,
                AlarmLevel::Level1,
                AlarmType::DelaminationRate,
                features.delamination_rate,
                self.config.delamination_rate_threshold,
            );

            log::warn!(
                "一级告警触发: {}-{} {} 分层扩展速率 {:.2} mm/h > 阈值 {:.1} mm/h",
                features.turbine_id,
                features.blade_id,
                features.section,
                features.delamination_rate,
                self.config.delamination_rate_threshold
            );

            self.push_alarm(alarm).await?;
            self.set_cooldown(key).await;
        }

        Ok(())
    }

    async fn check_frequency_offset(
        &self,
        features: &DamageFeatures,
        vibration_signal: Option<&[f64]>,
    ) -> Result<(), AlarmPusherError> {
        let rotor_speed = features.rotor_speed;
        let wind_speed = features.wind_speed;

        if !self.is_operating_condition_stable(rotor_speed, wind_speed) {
            log::debug!(
                "工况不稳定，跳过频率检查: 转速 {:.1} rpm, 风速 {:.1} m/s",
                rotor_speed, wind_speed
            );
            return Ok(());
        }

        let natural_frequency = if let Some(signal) = vibration_signal {
            match self.extract_natural_frequency(signal, rotor_speed) {
                Ok(freq) => freq,
                Err(e) => {
                    log::warn!("阶次跟踪提取频率失败: {}", e);
                    self.normalize_frequency_by_speed(features.natural_frequency, rotor_speed)
                }
            }
        } else {
            self.normalize_frequency_by_speed(features.natural_frequency, rotor_speed)
        };

        let offset = ((natural_frequency - self.baseline_frequency) / self.baseline_frequency).abs() * 100.0;

        let key = format!(
            "freq_{}_{}_{}",
            features.turbine_id, features.blade_id, features.section
        );

        self.add_frequency_history(&key, natural_frequency).await;
        let confirmed = self.confirm_frequency_trend(&key, offset).await;

        if confirmed && offset >= self.config.frequency_offset_threshold {
            if !self.is_in_cooldown(&key).await {
                let alarm = self.create_alarm(
                    features,
                    AlarmLevel::Level2,
                    AlarmType::FrequencyOffset,
                    offset,
                    self.config.frequency_offset_threshold,
                );

                log::warn!(
                    "二级告警触发: {}-{} {} 固有频率偏移 {:.1}% > 阈值 {:.1}% (转速 {:.1} rpm, 风速 {:.1} m/s)",
                    features.turbine_id,
                    features.blade_id,
                    features.section,
                    offset,
                    self.config.frequency_offset_threshold,
                    rotor_speed,
                    wind_speed
                );

                self.push_alarm(alarm).await?;
                self.set_cooldown(key).await;
            }
        }

        Ok(())
    }

    fn is_operating_condition_stable(&self, rotor_speed: f64, wind_speed: f64) -> bool {
        let speed_stable = rotor_speed >= self.config.valid_speed_range.0
            && rotor_speed <= self.config.valid_speed_range.1
            && (rotor_speed - self.config.baseline_rotor_speed).abs() < 3.0;

        let wind_stable = wind_speed >= self.config.valid_wind_range.0
            && wind_speed <= self.config.valid_wind_range.1;

        speed_stable && wind_stable
    }

    fn normalize_frequency_by_speed(&self, measured_freq: f64, rotor_speed: f64) -> f64 {
        measured_freq / (rotor_speed / self.config.baseline_rotor_speed).sqrt()
    }

    fn extract_natural_frequency(
        &self,
        vibration_signal: &[f64],
        rotor_speed: f64,
    ) -> Result<f64, SignalProcessingError> {
        let order_spectrum = self.order_tracker.compute_order_spectrum(
            vibration_signal,
            rotor_speed,
            None,
        )?;

        let (freq, _) = self.order_tracker.extract_natural_frequency(
            &order_spectrum,
            rotor_speed,
        )?;

        Ok(freq)
    }

    async fn add_frequency_history(&self, key: &str, frequency: f64) {
        let mut history = self.frequency_history.lock().await;
        let entry = history.entry(key.to_string()).or_insert_with(Vec::new);
        entry.push((Utc::now(), frequency));

        if entry.len() > self.config.frequency_history_size {
            entry.remove(0);
        }
    }

    async fn confirm_frequency_trend(&self, key: &str, current_offset: f64) -> bool {
        let history = self.frequency_history.lock().await;
        let entry = match history.get(key) {
            Some(e) => e,
            None => return false,
        };

        if entry.len() < self.config.trend_window_size {
            return false;
        }

        let recent = &entry[entry.len().saturating_sub(self.config.trend_window_size)..];
        let mut exceed_count = 0;
        let mut total_offset = 0.0;

        for (_, freq) in recent {
            let offset = ((freq - self.baseline_frequency) / self.baseline_frequency).abs() * 100.0;
            if offset >= self.config.frequency_offset_threshold {
                exceed_count += 1;
            }
            total_offset += offset;
        }

        let avg_offset = total_offset / recent.len() as f64;

        exceed_count >= self.config.trend_min_points
            && avg_offset >= self.config.frequency_offset_threshold * 0.9
            && current_offset >= self.config.frequency_offset_threshold
    }

    fn create_alarm(
        &self,
        features: &DamageFeatures,
        level: AlarmLevel,
        alarm_type: AlarmType,
        actual_value: f64,
        threshold: f64,
    ) -> Alarm {
        let message = match alarm_type {
            AlarmType::DelaminationRate => format!(
                "分层扩展速率超限：{}-{} {} 当前 {:.1} mm/h，阈值 {:.1} mm/h，损伤概率 {:.0}%",
                features.turbine_id, features.blade_id, features.section,
                actual_value, threshold, features.delamination_prob * 100.0
            ),
            AlarmType::FrequencyOffset => format!(
                "叶片固有频率偏移超限：{}-{} {} 当前偏移 {:.1}%，阈值 {:.1}%，健康度 {}",
                features.turbine_id, features.blade_id, features.section,
                actual_value, threshold, features.health_score
            ),
        };

        Alarm {
            id: format!("ALARM-{}-{}", Utc::now().timestamp(), uuid::Uuid::new_v4().simple()),
            turbine_id: features.turbine_id.clone(),
            blade_id: features.blade_id.clone(),
            section: features.section.clone(),
            alarm_level: match level {
                AlarmLevel::Level1 => "一级".to_string(),
                AlarmLevel::Level2 => "二级".to_string(),
            },
            alarm_type: match alarm_type {
                AlarmType::DelaminationRate => "delamination_rate".to_string(),
                AlarmType::FrequencyOffset => "frequency_offset".to_string(),
            },
            message,
            threshold,
            actual_value,
            timestamp: Utc::now(),
            acknowledged: 0,
            mes_pushed: 0,
        }
    }

    async fn push_alarm(&self, alarm: Alarm) -> Result<(), AlarmPusherError> {
        self.alarm_sender
            .send(alarm.clone())
            .await
            .map_err(|e| AlarmPusherError::ChannelSendError(e.to_string()))?;

        let mes_pusher = self.mes_pusher.clone();
        tokio::spawn(async move {
            match mes_pusher.push_alarm(&alarm).await {
                Ok(_) => {
                    log::info!("告警推送MES成功: {}", alarm.id);
                }
                Err(e) => {
                    log::error!("告警推送MES失败: {} - {}", alarm.id, e);
                }
            }
        });

        Ok(())
    }

    async fn is_in_cooldown(&self, key: &str) -> bool {
        let cooldowns = self.cooldown_periods.lock().await;
        if let Some(last_time) = cooldowns.get(key) {
            Utc::now() - *last_time < Duration::minutes(self.config.cooldown_minutes)
        } else {
            false
        }
    }

    async fn set_cooldown(&self, key: String) {
        let mut cooldowns = self.cooldown_periods.lock().await;
        cooldowns.insert(key, Utc::now());
    }

    pub async fn acknowledge_alarm(&self, alarm_id: &str) -> Result<(), AlarmPusherError> {
        log::info!("告警已确认: {}", alarm_id);
        Ok(())
    }

    pub async fn handle_message(
        &self,
        message: AlarmMessage,
    ) -> Result<(), AlarmPusherError> {
        match message {
            AlarmMessage::DamageFeatures(features) => {
                self.process_damage_features(features).await
            }
            AlarmMessage::VibrationSignal { features, signal } => {
                self.process_vibration_signal(&features, &signal).await
            }
            AlarmMessage::Acknowledge(alarm_id) => {
                self.acknowledge_alarm(&alarm_id).await
            }
            AlarmMessage::Shutdown => {
                log::info!("告警推送器收到关闭信号");
                Ok(())
            }
        }
    }
}

pub async fn start_alarm_pusher(
    pusher: Arc<AlarmPusher>,
    mut receiver: mpsc::Receiver<AlarmMessage>,
) -> Result<(), AlarmPusherError> {
    log::info!("告警推送器任务启动");

    while let Some(message) = receiver.recv().await {
        if let AlarmMessage::Shutdown = message {
            log::info!("告警推送器任务正常关闭");
            break;
        }

        if let Err(e) = pusher.handle_message(message).await {
            log::error!("告警推送器处理消息失败: {}", e);
        }
    }

    Ok(())
}
