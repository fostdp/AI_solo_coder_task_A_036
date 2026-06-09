use std::sync::Arc;
use chrono::Utc;
use uuid::Uuid;
use thiserror::Error;

use crate::models::damage::DamageFeatures;
use crate::models::alarm::{Alarm, AlarmLevel, AlarmType};
use crate::services::mes_pusher::MesPusher;
use crate::services::signal_processing::OrderTracker;
use ndarray::Array1;

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
    order_tracker: OrderTracker,
    baseline_rotor_speed: f64,
    valid_speed_range: (f64, f64),
    valid_wind_range: (f64, f64),
    frequency_history: std::collections::HashMap<String, Vec<(chrono::DateTime<Utc>, f64)>>,
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
            order_tracker: OrderTracker::new(1000.0, 10.0, 0.1),
            baseline_rotor_speed: 12.0,
            valid_speed_range: (9.0, 15.0),
            valid_wind_range: (3.0, 25.0),
            frequency_history: std::collections::HashMap::new(),
        }
    }

    pub fn check_frequency_with_conditions(
        &mut self,
        features: &DamageFeatures,
        vibration_signal: Option<&[f64]>,
    ) -> Result<(), AlarmError> {
        let rotor_speed = features.rotor_speed;
        let wind_speed = features.wind_speed;

        let speed_stable = rotor_speed >= self.valid_speed_range.0 
            && rotor_speed <= self.valid_speed_range.1
            && (rotor_speed - self.baseline_rotor_speed).abs() < 3.0;

        let wind_stable = wind_speed >= self.valid_wind_range.0 
            && wind_speed <= self.valid_wind_range.1;

        if !speed_stable || !wind_stable {
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
                    features.natural_frequency
                }
            }
        } else {
            self.normalize_frequency_by_speed(features.natural_frequency, rotor_speed)
        };

        let baseline_frequency = 12.5;
        let offset = ((natural_frequency - baseline_frequency) / baseline_frequency).abs() * 100.0;

        let key = format!(
            "freq_{}_{}_{}",
            features.turbine_id, features.blade_id, features.section
        );

        self.add_frequency_history(&key, natural_frequency);
        let confirmed = self.confirm_frequency_trend(&key, offset);

        if confirmed && offset >= self.frequency_offset_threshold {
            if !self.is_in_cooldown(&key) {
                let alarm = self.create_alarm(
                    features,
                    AlarmLevel::Level2,
                    AlarmType::FrequencyOffset,
                    offset,
                    self.frequency_offset_threshold,
                );

                log::warn!(
                    "二级告警触发: {}-{} {} 固有频率偏移 {:.1}% > 阈值 {:.1}% (转速 {:.1} rpm, 风速 {:.1} m/s)",
                    features.turbine_id,
                    features.blade_id,
                    features.section,
                    offset,
                    self.frequency_offset_threshold,
                    rotor_speed,
                    wind_speed
                );

                let rt = tokio::runtime::Handle::try_current()
                    .ok()
                    .unwrap_or_else(|| {
                        tokio::runtime::Runtime::new().unwrap().handle().clone()
                    });
                rt.spawn(async move {
                    let _ = self.push_to_mes(&alarm).await;
                });
                self.set_cooldown(key);
            }
        }

        Ok(())
    }

    fn extract_natural_frequency(
        &self,
        vibration_signal: &[f64],
        rotor_speed: f64,
    ) -> Result<f64, String> {
        let order_spectrum = self.order_tracker.compute_order_spectrum(
            vibration_signal,
            rotor_speed,
            None,
        ).map_err(|e| e.to_string())?;

        let (freq, _) = self.order_tracker.extract_natural_frequency(
            &order_spectrum,
            rotor_speed,
        ).map_err(|e| e.to_string())?;

        Ok(freq)
    }

    fn normalize_frequency_by_speed(&self, measured_freq: f64, rotor_speed: f64) -> f64 {
        let speed_ratio = self.baseline_rotor_speed / rotor_speed.max(0.1);
        measured_freq * speed_ratio.sqrt()
    }

    fn add_frequency_history(&mut self, key: &str, frequency: f64) {
        let history = self.frequency_history
            .entry(key.to_string())
            .or_insert_with(Vec::new);
        
        history.push((Utc::now(), frequency));
        
        if history.len() > 10 {
            history.remove(0);
        }
    }

    fn confirm_frequency_trend(&self, key: &str, current_offset: f64) -> bool {
        if let Some(history) = self.frequency_history.get(key) {
            if history.len() < 3 {
                return false;
            }

            let recent_offsets: Vec<f64> = history.iter()
                .rev()
                .take(5)
                .map(|(_, freq)| ((freq - 12.5) / 12.5).abs() * 100.0)
                .collect();

            let sustained_count = recent_offsets.iter()
                .filter(|&&offset| offset >= self.frequency_offset_threshold * 0.8)
                .count();

            let avg_offset: f64 = recent_offsets.iter().sum::<f64>() / recent_offsets.len() as f64;

            sustained_count >= 3 && avg_offset >= self.frequency_offset_threshold * 0.9
        } else {
            false
        }
    }

    pub fn is_operating_condition_stable(&self, rotor_speed: f64, wind_speed: f64) -> bool {
        let speed_ok = rotor_speed >= self.valid_speed_range.0 
            && rotor_speed <= self.valid_speed_range.1;
        let wind_ok = wind_speed >= self.valid_wind_range.0 
            && wind_speed <= self.valid_wind_range.1;
        let speed_stable = (rotor_speed - self.baseline_rotor_speed).abs() < 2.0;
        
        speed_ok && wind_ok && speed_stable
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
