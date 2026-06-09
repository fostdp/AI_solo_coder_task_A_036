use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use thiserror::Error;

use crate::models::{
    strain::StrainData,
    ae::AEEvent,
    damage::DamageFeatures,
};
use crate::services::signal_processing::{
    SignalProcessor, SignalProcessingError, ThresholdMethod,
};

#[derive(Error, Debug)]
pub enum EthernetDriverError {
    #[error("信号处理失败: {0}")]
    SignalProcessingError(#[from] SignalProcessingError),
    #[error("通道发送失败: {0}")]
    ChannelSendError(String),
    #[error("配置错误: {0}")]
    ConfigError(String),
}

#[derive(Debug, Clone)]
pub enum DriverMessage {
    StrainData(Vec<StrainData>),
    AEEvent(AEEvent, f64, f64),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct ProcessedAEData {
    pub event: AEEvent,
    pub denoised_amplitude: f64,
    pub normalized_duration: f64,
    pub normalized_frequency: f64,
    pub normalized_energy: f64,
    pub is_valid: bool,
    pub wind_speed: f64,
    pub rotor_speed: f64,
}

#[derive(Debug, Clone)]
pub struct ProcessedStrainData {
    pub data: StrainData,
    pub is_valid: bool,
}

pub struct EthernetDriver {
    signal_processor: Arc<SignalProcessor>,
    strain_sender: mpsc::Sender<ProcessedStrainData>,
    ae_sender: mpsc::Sender<ProcessedAEData>,
    damage_sender: mpsc::Sender<DamageFeatures>,
    wavelet_level: usize,
    threshold_method: ThresholdMethod,
    default_wind_speed: f64,
    default_rotor_speed: f64,
}

impl EthernetDriver {
    pub fn new(
        signal_processor: Arc<SignalProcessor>,
        strain_sender: mpsc::Sender<ProcessedStrainData>,
        ae_sender: mpsc::Sender<ProcessedAEData>,
        damage_sender: mpsc::Sender<DamageFeatures>,
    ) -> Self {
        Self {
            signal_processor,
            strain_sender,
            ae_sender,
            damage_sender,
            wavelet_level: 2,
            threshold_method: ThresholdMethod::Universal,
            default_wind_speed: 8.0,
            default_rotor_speed: 12.0,
        }
    }

    pub fn with_wavelet_level(mut self, level: usize) -> Self {
        self.wavelet_level = level;
        self
    }

    pub fn with_threshold_method(mut self, method: ThresholdMethod) -> Self {
        self.threshold_method = method;
        self
    }

    pub fn with_default_conditions(mut self, wind_speed: f64, rotor_speed: f64) -> Self {
        self.default_wind_speed = wind_speed;
        self.default_rotor_speed = rotor_speed;
        self
    }

    pub async fn process_strain_batch(
        &self,
        batch: Vec<StrainData>,
    ) -> Result<(), EthernetDriverError> {
        log::info!("以太网驱动收到应变数据，共{}条，开始预处理", batch.len());

        for data in batch {
            let wind_speed = data.wind_speed.unwrap_or(self.default_wind_speed);
            let rotor_speed = data.rotor_speed.unwrap_or(self.default_rotor_speed);

            let normalized_strain = self.signal_processor.normalize_by_wind_speed(
                data.strain_value,
                wind_speed,
                crate::services::signal_processing::SignalType::Amplitude,
            )?;

            let mut processed = ProcessedStrainData {
                data: data.clone(),
                is_valid: true,
            };
            processed.data.strain_value = normalized_strain;

            self.strain_sender
                .send(processed)
                .await
                .map_err(|e| EthernetDriverError::ChannelSendError(e.to_string()))?;
        }

        log::info!("应变数据预处理完成，已发送到插值模块");
        Ok(())
    }

    pub async fn process_ae_batch(
        &self,
        batch: Vec<AEEvent>,
    ) -> Result<(), EthernetDriverError> {
        log::info!("以太网驱动收到声发射事件，共{}条，开始预处理", batch.len());

        for event in batch {
            let wind_speed = event.wind_speed.unwrap_or(self.default_wind_speed);
            let rotor_speed = event.rotor_speed.unwrap_or(self.default_rotor_speed);

            let (denoised_amp, norm_dur, norm_freq, norm_energy) =
                self.signal_processor.denoise_ae_signal(
                    event.amplitude,
                    event.duration,
                    event.frequency_peak,
                    event.energy,
                    wind_speed,
                    rotor_speed,
                )?;

            let is_valid = self.signal_processor.adaptive_wind_speed_filter(
                denoised_amp,
                wind_speed,
                event.counts,
            );

            let processed = ProcessedAEData {
                event: event.clone(),
                denoised_amplitude: denoised_amp,
                normalized_duration: norm_dur,
                normalized_frequency: norm_freq,
                normalized_energy: norm_energy,
                is_valid,
                wind_speed,
                rotor_speed,
            };

            self.ae_sender
                .send(processed)
                .await
                .map_err(|e| EthernetDriverError::ChannelSendError(e.to_string()))?;
        }

        log::info!("声发射事件预处理完成，已发送到分类模块");
        Ok(())
    }

    pub async fn process_damage_features(
        &self,
        features: DamageFeatures,
    ) -> Result<(), EthernetDriverError> {
        log::info!(
            "以太网驱动收到损伤特征: {}-{} {}",
            features.turbine_id,
            features.blade_id,
            features.section
        );

        self.damage_sender
            .send(features)
            .await
            .map_err(|e| EthernetDriverError::ChannelSendError(e.to_string()))?;

        Ok(())
    }

    pub async fn handle_message(
        &self,
        message: DriverMessage,
    ) -> Result<(), EthernetDriverError> {
        match message {
            DriverMessage::StrainData(data) => self.process_strain_batch(data).await,
            DriverMessage::AEEvent(event, wind, rotor) => {
                let mut event = event;
                event.wind_speed = Some(wind);
                event.rotor_speed = Some(rotor);
                self.process_ae_batch(vec![event]).await
            }
            DriverMessage::Shutdown => {
                log::info!("以太网驱动收到关闭信号");
                Ok(())
            }
        }
    }
}

pub async fn start_ethernet_driver(
    driver: Arc<EthernetDriver>,
    mut receiver: mpsc::Receiver<DriverMessage>,
) -> Result<(), EthernetDriverError> {
    log::info!("以太网驱动任务启动");

    while let Some(message) = receiver.recv().await {
        if let DriverMessage::Shutdown = message {
            log::info!("以太网驱动任务正常关闭");
            break;
        }

        if let Err(e) = driver.handle_message(message).await {
            log::error!("以太网驱动处理消息失败: {}", e);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ae::AEEvent;
    use chrono::Utc;

    #[tokio::test]
    async fn test_ae_preprocessing() {
        let signal_processor = Arc::new(SignalProcessor::new());
        let (strain_tx, _strain_rx) = mpsc::channel::<ProcessedStrainData>(100);
        let (ae_tx, mut ae_rx) = mpsc::channel::<ProcessedAEData>(100);
        let (damage_tx, _damage_rx) = mpsc::channel::<DamageFeatures>(100);

        let driver = EthernetDriver::new(
            signal_processor,
            strain_tx,
            ae_tx,
            damage_tx,
        );

        let event = AEEvent {
            turbine_id: "WT001".to_string(),
            blade_id: "A".to_string(),
            sensor_id: "AE01".to_string(),
            section: "mid".to_string(),
            amplitude: 95.0,
            duration: 1500.0,
            frequency_peak: 250.0,
            frequency_center: 180.0,
            energy: 12500.0,
            counts: 45,
            rise_time: 250.0,
            wind_speed: Some(8.0),
            rotor_speed: Some(12.0),
            timestamp: Utc::now(),
        };

        driver.process_ae_batch(vec![event.clone()]).await.unwrap();

        let processed = ae_rx.recv().await.unwrap();
        assert!(processed.is_valid);
        assert!(processed.denoised_amplitude > 0.0);
        assert_eq!(processed.wind_speed, 8.0);
        assert_eq!(processed.rotor_speed, 12.0);
    }
}
