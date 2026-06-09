use serde::{Deserialize, Serialize};
use reqwest::Client;

use crate::models::alarm::Alarm;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MesAlarmPayload {
    pub alarm_id: String,
    pub turbine_id: String,
    pub blade_id: String,
    pub alarm_level: String,
    pub alarm_type: String,
    pub message: String,
    pub threshold: f64,
    pub actual_value: f64,
    pub timestamp: String,
    pub system_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MesResponse {
    pub success: bool,
    pub message: String,
    pub mes_reference_id: Option<String>,
}

pub struct MesPusher {
    client: Client,
    api_url: String,
    api_token: String,
    enabled: bool,
}

impl MesPusher {
    pub fn new() -> Self {
        let api_url = std::env::var("MES_API_URL")
            .unwrap_or_else(|_| "http://mes.example.com/api/alerts".to_string());
        let api_token = std::env::var("MES_API_TOKEN")
            .unwrap_or_else(|_| "default_token".to_string());
        let enabled = std::env::var("MES_ENABLED")
            .ok()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(true);

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self {
            client,
            api_url,
            api_token,
            enabled,
        }
    }

    pub async fn push_alarm(&self, alarm: &Alarm) -> Result<MesResponse, String> {
        if !self.enabled {
            log::info!("MES推送已禁用，跳过告警: {}", alarm.id);
            return Ok(MesResponse {
                success: true,
                message: "MES推送已禁用，本地记录成功".to_string(),
                mes_reference_id: Some(format!("LOCAL-{}", alarm.id)),
            });
        }

        let payload = MesAlarmPayload {
            alarm_id: alarm.id.clone(),
            turbine_id: alarm.turbine_id.clone(),
            blade_id: alarm.blade_id.clone(),
            alarm_level: alarm.alarm_level.clone(),
            alarm_type: alarm.alarm_type.clone(),
            message: alarm.message.clone(),
            threshold: alarm.threshold,
            actual_value: alarm.actual_value,
            timestamp: alarm.timestamp.to_rfc3339(),
            system_source: "WindTurbineBladeMonitor".to_string(),
        };

        log::debug!("推送告警到MES: {:?}", payload);

        match self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    match response.json::<MesResponse>().await {
                        Ok(mes_resp) => {
                            log::info!(
                                "告警推送MES成功: {}, MES引用ID: {:?}",
                                alarm.id,
                                mes_resp.mes_reference_id
                            );
                            Ok(mes_resp)
                        }
                        Err(e) => {
                            log::warn!("MES响应解析失败: {}", e);
                            Ok(MesResponse {
                                success: true,
                                message: format!("推送成功但响应解析失败: {}", e),
                                mes_reference_id: None,
                            })
                        }
                    }
                } else {
                    let error_msg = format!("MES返回错误状态码: {}", status);
                    log::error!("{}", error_msg);
                    Err(error_msg)
                }
            }
            Err(e) => {
                let error_msg = format!("MES推送请求失败: {}", e);
                log::error!("{}", error_msg);
                Err(error_msg)
            }
        }
    }

    pub async fn push_damage_report(
        &self,
        turbine_id: &str,
        blade_id: &str,
        damage_type: &str,
        severity: u8,
        recommendations: &str,
    ) -> Result<MesResponse, String> {
        if !self.enabled {
            return Ok(MesResponse {
                success: true,
                message: "MES推送已禁用，本地记录成功".to_string(),
                mes_reference_id: Some(format!("LOCAL-DMG-{}-{}", turbine_id, blade_id)),
            });
        }

        let payload = serde_json::json!({
            "type": "damage_report",
            "turbine_id": turbine_id,
            "blade_id": blade_id,
            "damage_type": damage_type,
            "severity_level": severity,
            "recommendations": recommendations,
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "system_source": "WindTurbineBladeMonitor",
        });

        match self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(response.json::<MesResponse>().await.unwrap_or_else(|_| MesResponse {
                        success: true,
                        message: "推送成功".to_string(),
                        mes_reference_id: None,
                    }))
                } else {
                    Err(format!("MES返回错误状态码: {}", response.status()))
                }
            }
            Err(e) => Err(format!("MES推送请求失败: {}", e)),
        }
    }

    pub async fn push_maintenance_task(
        &self,
        turbine_id: &str,
        blade_id: &str,
        task_type: &str,
        priority: &str,
        scheduled_date: Option<String>,
    ) -> Result<MesResponse, String> {
        if !self.enabled {
            return Ok(MesResponse {
                success: true,
                message: "MES推送已禁用，本地记录成功".to_string(),
                mes_reference_id: Some(format!("LOCAL-MAINT-{}-{}", turbine_id, blade_id)),
            });
        }

        let payload = serde_json::json!({
            "type": "maintenance_task",
            "turbine_id": turbine_id,
            "blade_id": blade_id,
            "task_type": task_type,
            "priority": priority,
            "scheduled_date": scheduled_date,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "system_source": "WindTurbineBladeMonitor",
        });

        match self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    Ok(response.json::<MesResponse>().await.unwrap_or_else(|_| MesResponse {
                        success: true,
                        message: "推送成功".to_string(),
                        mes_reference_id: None,
                    }))
                } else {
                    Err(format!("MES返回错误状态码: {}", response.status()))
                }
            }
            Err(e) => Err(format!("MES推送请求失败: {}", e)),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn get_api_url(&self) -> &str {
        &self.api_url
    }
}
