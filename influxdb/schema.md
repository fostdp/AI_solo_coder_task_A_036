# InfluxDB 数据结构设计

## 数据库：wind_turbine_blades

### 保留策略
- **raw_data**: 原始数据，保留90天
- **hourly_agg**: 小时聚合数据，保留365天
- **daily_agg**: 日聚合数据，保留3年

### 测量（Measurements）

#### 1. strain_data (应变分布数据)
**Tags**:
- turbine_id (风机编号: WT001-WT100)
- blade_id (叶片编号: A/B/C)
- sensor_id (传感器编号: S01-S20)
- section (叶片截面: root/mid/tip)

**Fields**:
- strain_value (应变值: microstrain)
- temperature (温度: °C)
- position_x, position_y, position_z (传感器位置坐标)

**Retention**: raw_data (90天)

#### 2. ae_events (声发射事件数据)
**Tags**:
- turbine_id
- blade_id
- sensor_id
- section

**Fields**:
- amplitude (幅值: dB)
- duration (持续时间: μs)
- frequency_peak (峰值频率: kHz)
- frequency_center (中心频率: kHz)
- energy (能量: aJ)
- counts (振铃计数)
- rise_time (上升时间: μs)

**Retention**: raw_data (90天)

#### 3. damage_features (损伤特征数据)
**Tags**:
- turbine_id
- blade_id
- section

**Fields**:
- matrix_cracking_prob (基体开裂概率)
- fiber_breakage_prob (纤维断裂概率)
- delamination_prob (分层概率)
- damage_severity (损伤严重程度: 0-100)
- natural_frequency (固有频率: Hz)
- delamination_rate (分层扩展速率: mm/h)
- health_score (健康度评分: 0-100)

**Retention**: raw_data (90天)

#### 4. blade_health (叶片健康度汇总)
**Tags**:
- turbine_id
- blade_id

**Fields**:
- health_score (健康度评分: 0-100)
- damage_type (损伤类型: none/matrix/fiber/delamination)
- severity_level (严重等级: 0-4)
- last_check (最后检查时间戳)

**Retention**: hourly_agg (365天)

#### 5. alarms (告警数据)
**Tags**:
- turbine_id
- blade_id
- alarm_level (一级/二级)
- alarm_type (delamination_rate/frequency_offset)

**Fields**:
- message (告警信息)
- threshold (阈值)
- actual_value (实际值)
- timestamp (告警时间)
- acknowledged (是否确认: 0/1)
- mes_pushed (是否推送MES: 0/1)

**Retention**: daily_agg (3年)

### 连续查询（CQ）
- **cq_strain_hourly**: 应变数据小时聚合
- **cq_strain_daily**: 应变数据日聚合
- **cq_ae_hourly**: 声发射事件小时聚合
