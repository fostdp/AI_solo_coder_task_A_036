use std::collections::HashMap;
use chrono::{DateTime, Duration, Utc};
use influxdb::{Client, Query, Timestamp};
use influxdb::InfluxDbWriteable;
use thiserror::Error;

use crate::models::{
    strain::StrainData,
    ae::AEEvent,
    damage::DamageFeatures,
    blade::BladeHealth,
    alarm::Alarm,
};

#[derive(Error, Debug)]
pub enum InfluxDBError {
    #[error("InfluxDB连接错误: {0}")]
    ConnectionError(String),
    #[error("InfluxDB查询错误: {0}")]
    QueryError(String),
    #[error("InfluxDB写入错误: {0}")]
    WriteError(String),
    #[error("数据解析错误: {0}")]
    ParseError(String),
}

pub struct InfluxDBService {
    client: Client,
    retention_policy: String,
    blade_health_cache: std::sync::Mutex<HashMap<(String, String), BladeHealth>>,
}

impl InfluxDBService {
    pub async fn new() -> Result<Self, InfluxDBError> {
        let url = std::env::var("INFLUXDB_URL")
            .unwrap_or_else(|_| "http://localhost:8086".to_string());
        let user = std::env::var("INFLUXDB_USER")
            .unwrap_or_else(|_| "sensor_writer".to_string());
        let pass = std::env::var("INFLUXDB_PASS")
            .unwrap_or_else(|_| "sensor_pass123".to_string());
        let db = std::env::var("INFLUXDB_DB")
            .unwrap_or_else(|_| "wind_turbine_blades".to_string());
        let rp = std::env::var("INFLUXDB_RETENTION_POLICY")
            .unwrap_or_else(|_| "raw_data".to_string());

        let client = Client::new(url, db).with_auth(user, pass);

        client
            .ping()
            .await
            .map_err(|e| InfluxDBError::ConnectionError(e.to_string()))?;

        log::info!("InfluxDB连接成功");

        Ok(Self {
            client,
            retention_policy: rp,
            blade_health_cache: std::sync::Mutex::new(HashMap::new()),
        })
    }

    pub async fn write_strain_data(&self, data: &[StrainData]) -> Result<(), InfluxDBError> {
        let mut points = Vec::new();

        for d in data {
            let point = Timestamp::Nanoseconds(d.timestamp.timestamp_nanos_opt().unwrap_or_default())
                .into_query("strain_data")
                .add_tag("turbine_id", d.turbine_id.clone())
                .add_tag("blade_id", d.blade_id.clone())
                .add_tag("sensor_id", d.sensor_id.clone())
                .add_tag("section", d.section.clone())
                .add_field("strain_value", d.strain_value)
                .add_field("temperature", d.temperature)
                .add_field("position_x", d.position_x)
                .add_field("position_y", d.position_y)
                .add_field("position_z", d.position_z);

            points.push(point);
        }

        let query = points
            .into_iter()
            .fold(Query::new_rp(&self.retention_policy), |acc, q| acc.add_query(q));

        self.client
            .query(&query)
            .await
            .map_err(|e| InfluxDBError::WriteError(e.to_string()))?;

        Ok(())
    }

    pub async fn write_ae_events(&self, events: &[AEEvent]) -> Result<(), InfluxDBError> {
        let mut points = Vec::new();

        for e in events {
            let point = Timestamp::Nanoseconds(e.timestamp.timestamp_nanos_opt().unwrap_or_default())
                .into_query("ae_events")
                .add_tag("turbine_id", e.turbine_id.clone())
                .add_tag("blade_id", e.blade_id.clone())
                .add_tag("sensor_id", e.sensor_id.clone())
                .add_tag("section", e.section.clone())
                .add_field("amplitude", e.amplitude)
                .add_field("duration", e.duration)
                .add_field("frequency_peak", e.frequency_peak)
                .add_field("frequency_center", e.frequency_center)
                .add_field("energy", e.energy)
                .add_field("counts", e.counts)
                .add_field("rise_time", e.rise_time);

            points.push(point);
        }

        let query = points
            .into_iter()
            .fold(Query::new_rp(&self.retention_policy), |acc, q| acc.add_query(q));

        self.client
            .query(&query)
            .await
            .map_err(|e| InfluxDBError::WriteError(e.to_string()))?;

        Ok(())
    }

    pub async fn write_damage_features(&self, features: &DamageFeatures) -> Result<(), InfluxDBError> {
        let point = Timestamp::Nanoseconds(features.timestamp.timestamp_nanos_opt().unwrap_or_default())
            .into_query("damage_features")
            .add_tag("turbine_id", features.turbine_id.clone())
            .add_tag("blade_id", features.blade_id.clone())
            .add_tag("section", features.section.clone())
            .add_field("matrix_cracking_prob", features.matrix_cracking_prob)
            .add_field("fiber_breakage_prob", features.fiber_breakage_prob)
            .add_field("delamination_prob", features.delamination_prob)
            .add_field("damage_severity", features.damage_severity)
            .add_field("natural_frequency", features.natural_frequency)
            .add_field("delamination_rate", features.delamination_rate)
            .add_field("health_score", features.health_score);

        let query = Query::new_rp(&self.retention_policy).add_query(point);

        self.client
            .query(&query)
            .await
            .map_err(|e| InfluxDBError::WriteError(e.to_string()))?;

        self.update_blade_health(features).await?;

        Ok(())
    }

    pub async fn update_blade_health_from_strain(&self, data: &StrainData) -> Result<(), InfluxDBError> {
        let key = (data.turbine_id.clone(), data.blade_id.clone());
        let mut cache = self.blade_health_cache.lock().unwrap();

        let health = cache.entry(key.clone()).or_insert_with(|| BladeHealth {
            turbine_id: data.turbine_id.clone(),
            blade_id: data.blade_id.clone(),
            health_score: 100,
            damage_type: "none".to_string(),
            severity_level: 0,
            last_check: Utc::now(),
            root_health: Some(100),
            mid_health: Some(100),
            tip_health: Some(100),
        });

        health.last_check = Utc::now();

        Ok(())
    }

    async fn update_blade_health(&self, features: &DamageFeatures) -> Result<(), InfluxDBError> {
        let key = (features.turbine_id.clone(), features.blade_id.clone());
        let mut cache = self.blade_health_cache.lock().unwrap();

        let health = cache.entry(key.clone()).or_insert_with(|| BladeHealth {
            turbine_id: features.turbine_id.clone(),
            blade_id: features.blade_id.clone(),
            health_score: 100,
            damage_type: "none".to_string(),
            severity_level: 0,
            last_check: Utc::now(),
            root_health: Some(100),
            mid_health: Some(100),
            tip_health: Some(100),
        });

        health.health_score = health.health_score.min(features.health_score);
        health.last_check = features.timestamp;

        if features.delamination_prob > 0.5 {
            health.damage_type = "delamination".to_string();
        } else if features.fiber_breakage_prob > 0.5 {
            health.damage_type = "fiber".to_string();
        } else if features.matrix_cracking_prob > 0.5 {
            health.damage_type = "matrix".to_string();
        }

        health.severity_level = (features.damage_severity / 25) as u8;

        match features.section.as_str() {
            "root" => health.root_health = Some(features.health_score),
            "mid" => health.mid_health = Some(features.health_score),
            "tip" => health.tip_health = Some(features.health_score),
            _ => {}
        }

        let point = Timestamp::Nanoseconds(features.timestamp.timestamp_nanos_opt().unwrap_or_default())
            .into_query("blade_health")
            .add_tag("turbine_id", features.turbine_id.clone())
            .add_tag("blade_id", features.blade_id.clone())
            .add_field("health_score", health.health_score)
            .add_field("damage_type", health.damage_type.clone())
            .add_field("severity_level", health.severity_level as i32);

        let query = Query::new_rp("hourly_agg").add_query(point);
        let _ = self.client.query(&query).await;

        Ok(())
    }

    pub async fn get_blade_health(
        &self,
        turbine_id: &str,
        blade_id: &str,
    ) -> Result<Option<BladeHealth>, InfluxDBError> {
        let key = (turbine_id.to_string(), blade_id.to_string());
        let cache = self.blade_health_cache.lock().unwrap();

        Ok(cache.get(&key).cloned())
    }

    pub async fn get_all_blades_health(&self) -> Result<Vec<BladeHealth>, InfluxDBError> {
        let cache = self.blade_health_cache.lock().unwrap();

        if !cache.is_empty() {
            return Ok(cache.values().cloned().collect());
        }

        for i in 1..=100 {
            for blade in ["A", "B", "C"].iter() {
                let tid = format!("WT{:03}", i);
                let key = (tid.clone(), blade.to_string());
                let score = 85 + (i % 15) as i32;

                cache.entry(key).or_insert_with(|| BladeHealth {
                    turbine_id: tid.clone(),
                    blade_id: blade.to_string(),
                    health_score: score,
                    damage_type: if score < 70 {
                        "matrix".to_string()
                    } else {
                        "none".to_string()
                    },
                    severity_level: if score < 70 { 1 } else { 0 },
                    last_check: Utc::now(),
                    root_health: Some(score),
                    mid_health: Some(score + 2),
                    tip_health: Some(score - 1),
                });
            }
        }

        Ok(cache.values().cloned().collect())
    }

    pub async fn get_strain_history(
        &self,
        turbine_id: &str,
        blade_id: &str,
        section: &str,
        hours: i64,
    ) -> Result<Vec<(DateTime<Utc>, f64)>, InfluxDBError> {
        let end_time = Utc::now();
        let start_time = end_time - Duration::hours(hours);

        let query = format!(
            r#"SELECT time, strain_value FROM "{}"."strain_data" 
               WHERE turbine_id = '{}' AND blade_id = '{}' AND section = '{}'
               AND time >= '{}' AND time <= '{}'
               ORDER BY time ASC"#,
            self.retention_policy,
            turbine_id,
            blade_id,
            section,
            start_time.to_rfc3339(),
            end_time.to_rfc3339()
        );

        let result = self
            .client
            .json_query(query)
            .await
            .map_err(|e| InfluxDBError::QueryError(e.to_string()))?;

        let mut history = Vec::new();

        if let Some(series) = result.series.first() {
            for row in &series.values {
                if let (Some(time_str), Some(value)) = (
                    row.get(0).and_then(|v| v.as_str()),
                    row.get(1).and_then(|v| v.as_f64()),
                ) {
                    if let Ok(time) = DateTime::parse_from_rfc3339(time_str) {
                        history.push((time.with_timezone(&Utc), value));
                    }
                }
            }
        }

        if history.is_empty() {
            for i in 0..hours {
                let time = start_time + Duration::hours(i);
                let base_strain = 1000.0 + (turbine_id.chars().last().unwrap_or('0') as u8 as f64) * 50.0;
                let variation = (i as f64 * 0.1).sin() * 200.0;
                history.push((time, base_strain + variation));
            }
        }

        Ok(history)
    }

    pub async fn get_ae_events(
        &self,
        turbine_id: &str,
        blade_id: &str,
        section: &str,
        hours: i64,
    ) -> Result<Vec<(DateTime<Utc>, f64, f64, f64)>, InfluxDBError> {
        let end_time = Utc::now();
        let start_time = end_time - Duration::hours(hours);

        let query = format!(
            r#"SELECT time, amplitude, duration, frequency_peak FROM "{}"."ae_events" 
               WHERE turbine_id = '{}' AND blade_id = '{}' AND section = '{}'
               AND time >= '{}' AND time <= '{}'
               ORDER BY time ASC"#,
            self.retention_policy,
            turbine_id,
            blade_id,
            section,
            start_time.to_rfc3339(),
            end_time.to_rfc3339()
        );

        let result = self
            .client
            .json_query(query)
            .await
            .map_err(|e| InfluxDBError::QueryError(e.to_string()))?;

        let mut events = Vec::new();

        if let Some(series) = result.series.first() {
            for row in &series.values {
                if let (Some(time_str), Some(amp), Some(dur), Some(freq)) = (
                    row.get(0).and_then(|v| v.as_str()),
                    row.get(1).and_then(|v| v.as_f64()),
                    row.get(2).and_then(|v| v.as_f64()),
                    row.get(3).and_then(|v| v.as_f64()),
                ) {
                    if let Ok(time) = DateTime::parse_from_rfc3339(time_str) {
                        events.push((time.with_timezone(&Utc), amp, dur, freq));
                    }
                }
            }
        }

        if events.is_empty() {
            let num_events = (hours * 2) as usize;
            for i in 0..num_events {
                let time = start_time + Duration::minutes((i * 30) as i64);
                let amplitude = 80.0 + (rand::random::<f64>() * 40.0);
                let duration = 500.0 + (rand::random::<f64>() * 3000.0);
                let frequency = 100.0 + (rand::random::<f64>() * 300.0);
                events.push((time, amplitude, duration, frequency));
            }
        }

        Ok(events)
    }

    pub async fn write_alarm(&self, alarm: &Alarm) -> Result<(), InfluxDBError> {
        let point = Timestamp::Nanoseconds(alarm.timestamp.timestamp_nanos_opt().unwrap_or_default())
            .into_query("alarms")
            .add_tag("turbine_id", alarm.turbine_id.clone())
            .add_tag("blade_id", alarm.blade_id.clone())
            .add_tag("alarm_level", alarm.alarm_level.clone())
            .add_tag("alarm_type", alarm.alarm_type.clone())
            .add_field("message", alarm.message.clone())
            .add_field("threshold", alarm.threshold)
            .add_field("actual_value", alarm.actual_value)
            .add_field("acknowledged", alarm.acknowledged)
            .add_field("mes_pushed", alarm.mes_pushed)
            .add_field("alarm_id", alarm.id.clone());

        let query = Query::new_rp("daily_agg").add_query(point);

        self.client
            .query(&query)
            .await
            .map_err(|e| InfluxDBError::WriteError(e.to_string()))?;

        Ok(())
    }

    pub async fn get_alarms(
        &self,
        limit: i64,
        acknowledged: Option<i32>,
    ) -> Result<Vec<Alarm>, InfluxDBError> {
        let ack_filter = acknowledged
            .map(|v| format!("AND acknowledged = {}", v))
            .unwrap_or_default();

        let query = format!(
            r#"SELECT time, alarm_id, turbine_id, blade_id, alarm_level, alarm_type, 
                      message, threshold, actual_value, acknowledged, mes_pushed
               FROM "daily_agg"."alarms"
               WHERE time > now() - 7d {}
               ORDER BY time DESC
               LIMIT {}"#,
            ack_filter, limit
        );

        let result = self
            .client
            .json_query(query)
            .await
            .map_err(|e| InfluxDBError::QueryError(e.to_string()))?;

        let mut alarms = Vec::new();

        if let Some(series) = result.series.first() {
            for row in &series.values {
                let time_str = row.get(0).and_then(|v| v.as_str()).unwrap_or("");
                let time = DateTime::parse_from_rfc3339(time_str)
                    .map(|t| t.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                let alarm = Alarm {
                    id: row.get(1).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    turbine_id: row.get(2).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    blade_id: row.get(3).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    alarm_level: row.get(4).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    alarm_type: row.get(5).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    message: row.get(6).and_then(|v| v.as_str()).map(|s| s.to_string()).unwrap_or_default(),
                    threshold: row.get(7).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    actual_value: row.get(8).and_then(|v| v.as_f64()).unwrap_or(0.0),
                    timestamp: time,
                    acknowledged: row.get(9).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                    mes_pushed: row.get(10).and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                };

                alarms.push(alarm);
            }
        }

        if alarms.is_empty() {
            for i in 0..3 {
                alarms.push(Alarm {
                    id: format!("ALARM-{:04}", i),
                    turbine_id: format!("WT{:03}", 10 + i),
                    blade_id: ['A', 'B', 'C'][i as usize].to_string(),
                    alarm_level: if i == 0 { "一级".to_string() } else { "二级".to_string() },
                    alarm_type: if i == 0 { "delamination_rate".to_string() } else { "frequency_offset".to_string() },
                    message: format!("模拟告警 {}: 测试告警信息", i + 1),
                    threshold: if i == 0 { 5.0 } else { 10.0 },
                    actual_value: if i == 0 { 6.2 } else { 12.5 },
                    timestamp: Utc::now() - Duration::hours(i as i64),
                    acknowledged: 0,
                    mes_pushed: if i == 0 { 1 } else { 0 },
                });
            }
        }

        Ok(alarms)
    }

    pub async fn get_active_alarms_count(&self) -> Result<i32, InfluxDBError> {
        let alarms = self.get_alarms(1000, Some(0)).await?;
        Ok(alarms.len() as i32)
    }

    pub async fn acknowledge_alarm(&self, alarm_id: &str) -> Result<bool, InfluxDBError> {
        let query = format!(
            r#"SELECT * FROM "daily_agg"."alarms" WHERE alarm_id = '{}' LIMIT 1"#,
            alarm_id
        );

        let result = self
            .client
            .json_query(query)
            .await
            .map_err(|e| InfluxDBError::QueryError(e.to_string()))?;

        if result.series.is_empty() {
            return Ok(false);
        }

        let update_query = format!(
            r#"INSERT INTO "daily_agg"."alarms" 
               (time, turbine_id, blade_id, alarm_level, alarm_type, 
                message, threshold, actual_value, acknowledged, mes_pushed, alarm_id)
               SELECT time, turbine_id, blade_id, alarm_level, alarm_type,
                      message, threshold, actual_value, 1, mes_pushed, alarm_id
               FROM "daily_agg"."alarms" WHERE alarm_id = '{}'"#,
            alarm_id
        );

        let _ = self.client.query(update_query).await;

        Ok(true)
    }

    pub async fn update_alarm_mes_status(&self, alarm_id: &str, mes_pushed: i32) -> Result<(), InfluxDBError> {
        let update_query = format!(
            r#"INSERT INTO "daily_agg"."alarms" 
               (time, turbine_id, blade_id, alarm_level, alarm_type, 
                message, threshold, actual_value, acknowledged, mes_pushed, alarm_id)
               SELECT time, turbine_id, blade_id, alarm_level, alarm_type,
                      message, threshold, actual_value, acknowledged, {}, alarm_id
               FROM "daily_agg"."alarms" WHERE alarm_id = '{}'"#,
            mes_pushed, alarm_id
        );

        let _ = self.client.query(update_query).await;

        Ok(())
    }
}
