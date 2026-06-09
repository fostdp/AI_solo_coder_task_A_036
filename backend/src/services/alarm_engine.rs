use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;
use thiserror::Error;

use crate::models::damage::DamageFeatures;
use crate::models::alarm::{Alarm, AlarmLevel, AlarmType};
use crate::services::mes_pusher::MesPusher;

#[derive(Error, Debug)]
pub enum AlarmError {
    #[error("告警写入失败: {0}")]
    WriteError(String),
    #[error("MES推送失败: {0}")]
    MesPushError(String),
}

pub struct AlarmEngine {
    delamination_rate_threshold: f64,
    frequency_offset_threshold: f64,
    mes_pusher: Arc<MesPusher>,
    cooldown_periods: std::collections::HashMap<String, chrono::DateTime<Utc>>,
}

impl AlarmEngine {
    pub fn new(mes_pusher: Arc<MesPusher>) -> Self {
        let delam_threshold = std::env::var("ALARM_DELAMINATION_RATE_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(5.0);

        let freq_threshold = std::env::var("ALARM_FREQUENCY_OFFSET_THRESHOLD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);

        Self {
            delamination_rate_threshold: delam_threshold,
            frequency_offset_threshold: freq_threshold,
            mes_pusher,
            cooldown_periods: std::collections::HashMap::new(),
        }
    }

    pub async fn check_and_trigger_alarm(
        &mut self,
        features: &DamageFeatures,
    ) -> Result<(), AlarmError> {
        self.check_delamination_rate(features).await?;
        self.check_frequency_offset(features).await?;
        Ok(())
    }

    async fn check_delamination_rate(
        &mut self,
        features: &DamageFeatures,
    ) -> Result<(), AlarmError> {
        if features.delamination_rate >= self.delamination_rate_threshold {
            let key = format!(
                "delam_{}_{}_{}",
                features.turbine_id, features.blade_id, features.section
            );

            if !self.is_in_cooldown(&key) {
                let alarm = self.create_alarm(
                    features,
                    AlarmLevel::Level1,
                    AlarmType::DelaminationRate,
                    features.delamination_rate,
                    self.delamination_rate_threshold,
                );

                log::warn!(
                    "一级告警触发: {}-{} {} 分层扩展速率 {:.2} mm/h > 阈值 {:.2} mm/h",
                    features.turbine_id,
                    features.blade_id,
                    features.section,
                    features.delamination_rate,
                    self.delamination_rate_threshold
                );

                self.push_to_mes(&alarm).await?;
                self.set_cooldown(key);
            }
        }

        Ok(())
    }

    async fn check_frequency_offset(
        &mut self,
        features: &DamageFeatures,
    ) -> Result<(), AlarmError> {
        let baseline_frequency = 12.5;
        let offset = ((features.natural_frequency - baseline_frequency) / baseline_frequency).abs() * 100.0;

        if offset >= self.frequency_offset_threshold {
            let key = format!(
                "freq_{}_{}_{}",
                features.turbine_id, features.blade_id, features.section
            );

            if !self.is_in_cooldown(&key) {
                let alarm = self.create_alarm(
                    features,
                    AlarmLevel::Level2,
                    AlarmType::FrequencyOffset,
                    offset,
                    self.frequency_offset_threshold,
                );

                log::warn!(
                    "二级告警触发: {}-{} {} 固有频率偏移 {:.1}% > 阈值 {:.1}%",
                    features.turbine_id,
                    features.blade_id,
                    features.section,
                    offset,
                    self.frequency_offset_threshold
                );

                self.push_to_mes(&alarm).await?;
                self.set_cooldown(key);
            }
        }

        Ok(())
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
                "分层扩展速率超限：{}-{} {} 当前 {:.2} mm/h，阈值 {:.2} mm/h，损伤概率 {:.1}%",
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
            id: format!("ALARM-{}", Uuid::new_v4()),
            turbine_id: features.turbine_id.clone(),
            blade_id: features.blade_id.clone(),
            alarm_level: level.to_string(),
            alarm_type: alarm_type.to_string(),
            message,
            threshold,
            actual_value,
            timestamp: Utc::now(),
            acknowledged: 0,
            mes_pushed: 0,
        }
    }

    async fn push_to_mes(&self, alarm: &Alarm) -> Result<(), AlarmError> {
        match self.mes_pusher.push_alarm(alarm).await {
            Ok(_) => {
                let mut alarm_clone = alarm.clone();
                alarm_clone.mes_pushed = 1;
                log::info!("告警已成功推送至MES系统: {}", alarm.id);
                Ok(())
            }
            Err(e) => {
                log::error!("告警推送MES失败: {}, 错误: {}", alarm.id, e);
                Err(AlarmError::MesPushError(e))
            }
        }
    }

    fn is_in_cooldown(&self, key: &str) -> bool {
        if let Some(last_time) = self.cooldown_periods.get(key) {
            let cooldown_duration = chrono::Duration::minutes(30);
            Utc::now() - *last_time < cooldown_duration
        } else {
            false
        }
    }

    fn set_cooldown(&mut self, key: String) {
        self.cooldown_periods.insert(key, Utc::now());
    }

    pub fn cleanup_expired_cooldowns(&mut self) {
        let cooldown_duration = chrono::Duration::hours(1);
        let now = Utc::now();

        self.cooldown_periods
            .retain(|_, last_time| now - *last_time < cooldown_duration);
    }

    pub fn get_delamination_threshold(&self) -> f64 {
        self.delamination_rate_threshold
    }

    pub fn get_frequency_threshold(&self) -> f64 {
        self.frequency_offset_threshold
    }

    pub fn set_delamination_threshold(&mut self, threshold: f64) {
        self.delamination_rate_threshold = threshold;
    }

    pub fn set_frequency_threshold(&mut self, threshold: f64) {
        self.frequency_offset_threshold = threshold;
    }
}
