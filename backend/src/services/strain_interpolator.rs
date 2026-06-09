use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use thiserror::Error;
use serde::Deserialize;

use crate::models::strain::StrainData;
use crate::services::signal_processing::{
    KrigingInterpolator, StrainReading, StrainField, SignalProcessingError,
    VariogramModel,
};
use crate::services::ethernet_driver::ProcessedStrainData;

#[derive(Error, Debug)]
pub enum StrainInterpolatorError {
    #[error("信号处理失败: {0}")]
    SignalProcessingError(#[from] SignalProcessingError),
    #[error("通道发送失败: {0}")]
    ChannelSendError(String),
    #[error("插值失败: {0}")]
    InterpolationError(String),
    #[error("配置错误: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct InterpolationConfig {
    pub grid_resolution: usize,
    pub variogram_model: String,
    pub blade_length: f64,
    pub blade_chord: f64,
}

impl Default for InterpolationConfig {
    fn default() -> Self {
        Self {
            grid_resolution: 32,
            variogram_model: "Spherical".to_string(),
            blade_length: 20.0,
            blade_chord: 2.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InterpolatedStrainField {
    pub turbine_id: String,
    pub blade_id: String,
    pub strain_field: StrainField,
    pub sensor_count: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub wind_speed: f64,
    pub rotor_speed: f64,
}

pub struct StrainInterpolator {
    interpolator: KrigingInterpolator,
    config: Arc<InterpolationConfig>,
    field_sender: mpsc::Sender<InterpolatedStrainField>,
    sensor_readings: Arc<Mutex<std::collections::HashMap<(String, String), Vec<StrainReading>>>>,
    last_update: Arc<Mutex<std::collections::HashMap<(String, String), chrono::DateTime<chrono::Utc>>>>,
    auto_trigger_threshold: usize,
}

impl StrainInterpolator {
    pub fn new(
        config: Arc<InterpolationConfig>,
        field_sender: mpsc::Sender<InterpolatedStrainField>,
    ) -> Result<Self, StrainInterpolatorError> {
        let variogram_model = match config.variogram_model.as_str() {
            "Spherical" => VariogramModel::Spherical,
            "Exponential" => VariogramModel::Exponential,
            "Gaussian" => VariogramModel::Gaussian,
            "Linear" => VariogramModel::Linear,
            other => return Err(StrainInterpolatorError::ConfigError(
                format!("不支持的变异函数模型: {}", other)
            )),
        };

        let mut interpolator = KrigingInterpolator::new();
        interpolator.set_variogram_model(variogram_model);

        Ok(Self {
            interpolator,
            config,
            field_sender,
            sensor_readings: Arc::new(Mutex::new(std::collections::HashMap::new())),
            last_update: Arc::new(Mutex::new(std::collections::HashMap::new())),
            auto_trigger_threshold: 10,
        })
    }

    pub fn with_auto_trigger_threshold(mut self, threshold: usize) -> Self {
        self.auto_trigger_threshold = threshold;
        self
    }

    pub async fn process_strain_data(
        &self,
        data: ProcessedStrainData,
    ) -> Result<(), StrainInterpolatorError> {
        let key = (data.data.turbine_id.clone(), data.data.blade_id.clone());

        let reading = StrainReading {
            sensor_id: data.data.sensor_id.clone(),
            position_z: data.data.position_z,
            position_y: data.data.position_y,
            strain_value: data.data.strain_value,
            temperature: data.data.temperature,
            timestamp: data.data.timestamp,
        };

        let mut readings = self.sensor_readings.lock().await;
        let entry = readings.entry(key.clone()).or_insert_with(Vec::new);
        entry.push(reading);

        let wind_speed = data.data.wind_speed.unwrap_or(8.0);
        let rotor_speed = data.data.rotor_speed.unwrap_or(12.0);
        let timestamp = data.data.timestamp;

        drop(readings);

        let mut last_update = self.last_update.lock().await;
        last_update.insert(key.clone(), timestamp);
        drop(last_update);

        let readings = self.sensor_readings.lock().await;
        let should_interpolate = readings.get(&key)
            .map(|r| r.len() >= self.auto_trigger_threshold)
            .unwrap_or(false);
        drop(readings);

        if should_interpolate {
            self.interpolate_and_send(
                &key.0,
                &key.1,
                wind_speed,
                rotor_speed,
                timestamp,
            ).await?;

            let mut readings = self.sensor_readings.lock().await;
            if let Some(entry) = readings.get_mut(&key) {
                entry.clear();
            }
        }

        Ok(())
    }

    pub async fn interpolate_and_send(
        &self,
        turbine_id: &str,
        blade_id: &str,
        wind_speed: f64,
        rotor_speed: f64,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), StrainInterpolatorError> {
        let key = (turbine_id.to_string(), blade_id.to_string());

        let readings = self.sensor_readings.lock().await;
        let sensor_readings = readings.get(&key)
            .cloned()
            .unwrap_or_default();
        let sensor_count = sensor_readings.len();

        if sensor_count < 3 {
            return Err(StrainInterpolatorError::InterpolationError(
                format!("传感器数量不足，需要至少3个，当前{}个", sensor_count)
            ));
        }

        let known_points: Vec<(f64, f64, f64)> = sensor_readings
            .iter()
            .map(|r| (r.position_z, r.position_y, r.strain_value))
            .collect();

        let mut interpolator = self.interpolator.clone();
        let _ = interpolator.fit_variogram(&known_points);

        let strain_field = interpolator.interpolate_strain_field(
            &sensor_readings,
            self.config.grid_resolution,
            self.config.blade_length,
            self.config.blade_chord,
        )?;

        let interpolated = InterpolatedStrainField {
            turbine_id: turbine_id.to_string(),
            blade_id: blade_id.to_string(),
            strain_field,
            sensor_count,
            timestamp,
            wind_speed,
            rotor_speed,
        };

        self.field_sender
            .send(interpolated)
            .await
            .map_err(|e| StrainInterpolatorError::ChannelSendError(e.to_string()))?;

        log::info!(
            "应变插值完成: {}-{} 分辨率{}×{} 使用{}个传感器",
            turbine_id,
            blade_id,
            self.config.grid_resolution,
            self.config.grid_resolution,
            sensor_count
        );

        Ok(())
    }

    pub async fn get_strain_at_position(
        &self,
        turbine_id: &str,
        blade_id: &str,
        position_z: f64,
        position_y: f64,
    ) -> Result<Option<f64>, StrainInterpolatorError> {
        let key = (turbine_id.to_string(), blade_id.to_string());

        let readings = self.sensor_readings.lock().await;
        let sensor_readings = readings.get(&key)
            .cloned()
            .unwrap_or_default();

        if sensor_readings.len() < 3 {
            return Ok(None);
        }

        let known_points: Vec<(f64, f64, f64)> = sensor_readings
            .iter()
            .map(|r| (r.position_z, r.position_y, r.strain_value))
            .collect();

        let query_points = vec![(position_z, position_y)];
        let mut interpolator = self.interpolator.clone();
        let _ = interpolator.fit_variogram(&known_points);
        let results = interpolator.interpolate(&known_points, &query_points)?;

        Ok(results.first().map(|(_, val)| val))
    }

    pub async fn clear_readings(&self, turbine_id: &str, blade_id: &str) {
        let key = (turbine_id.to_string(), blade_id.to_string());
        let mut readings = self.sensor_readings.lock().await;
        readings.remove(&key);
        let mut last_update = self.last_update.lock().await;
        last_update.remove(&key);
    }

    pub async fn get_sensor_count(&self, turbine_id: &str, blade_id: &str) -> usize {
        let key = (turbine_id.to_string(), blade_id.to_string());
        self.sensor_readings.lock().await
            .get(&key)
            .map(|r| r.len())
            .unwrap_or(0)
    }
}

pub async fn start_strain_interpolator(
    interpolator: Arc<StrainInterpolator>,
    mut receiver: mpsc::Receiver<ProcessedStrainData>,
) -> Result<(), StrainInterpolatorError> {
    log::info!("应变插值器任务启动");

    while let Some(data) = receiver.recv().await {
        if let Err(e) = interpolator.process_strain_data(data).await {
            log::error!("应变插值器处理数据失败: {}", e);
        }
    }

    log::info!("应变插值器任务正常关闭");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_interpolation_process() {
        let config = Arc::new(InterpolationConfig::default());
        let (field_tx, mut field_rx) = mpsc::channel::<InterpolatedStrainField>(10);

        let interpolator = Arc::new(StrainInterpolator::new(config, field_tx).unwrap());

        for i in 0..15 {
            let data = ProcessedStrainData {
                data: StrainData {
                    turbine_id: "WT001".to_string(),
                    blade_id: "A".to_string(),
                    sensor_id: format!("S{:02}", i),
                    section: "mid".to_string(),
                    strain_value: 800.0 + i as f64 * 50.0,
                    temperature: 25.0,
                    position_x: 0.0,
                    position_y: (i as f64 - 7.5) * 0.15,
                    position_z: (i as f64 - 7.5) * 1.2,
                    wind_speed: Some(8.0),
                    rotor_speed: Some(12.0),
                    timestamp: Utc::now(),
                },
                is_valid: true,
            };

            interpolator.process_strain_data(data).await.unwrap();
        }

        let result = field_rx.recv().await.unwrap();
        assert_eq!(result.turbine_id, "WT001");
        assert_eq!(result.blade_id, "A");
        assert!(result.sensor_count >= 10);
    }
}
