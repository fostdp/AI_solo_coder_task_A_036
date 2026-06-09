# 大型风力发电机组叶片状态监测与修复决策系统

## 📋 目录

- [系统架构](#系统架构)
- [技术栈](#技术栈)
- [快速开始](#快速开始)
  - [Docker 一键部署](#docker-一键部署)
  - [本地开发环境](#本地开发环境)
- [传感器模拟器](#传感器模拟器)
  - [配置参数](#配置参数)
  - [交互命令](#交互命令)
  - [损伤注入](#损伤注入)
- [API 接口](#api-接口)
- [监控指标](#监控指标)
- [数据存储](#数据存储)
  - [保留策略](#保留策略)
  - [降采样](#降采样)
- [配置说明](#配置说明)
- [目录结构](#目录结构)

---

## 系统架构

### 整体架构图

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              前端 (SPA)                                  │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐  │
│  │ Blade3DViewer   │  │ HealthDashboard │  │  Chart.js (应变/声发射) │  │
│  │ Three.js 热力图 │  │ 健康度排行/统计 │  │   损伤概率曲线         │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘  │
│                                 ↓ Gzip 压缩                              │
└─────────────────────────────────┬───────────────────────────────────────┘
                                  │
                      ┌───────────▼───────────┐
                      │   Rust Backend (Axum) │
                      │  HTTP API :8000       │
                      │  /metrics (Prometheus)│
                      │  /health 健康检查      │
                      └───────────┬───────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │         tokio::mpsc channel 通信              │
          ▼                       ▼                       ▼
┌──────────────────┐  ┌──────────────────┐  ┌──────────────────┐
│ EthernetDriver   │  │ DamageClassifier │  │ StrainInterpolator│
│ 数据采集/预处理   │  │ 特征提取/随机森林 │  │ 克里金空间插值    │
│ 小波去噪         │  │ 损伤分类         │  │ 应变场生成       │
│ 工况归一化       │  │ 健康度评估       │  │ 梯度计算         │
└─────────┬────────┘  └─────────┬────────┘  └──────────────────┘
          │                       │
          ▼                       ▼
┌───────────────────────────────────────────┐
│              DamageFeatures               │
└───────────────────┬───────────────────────┘
                    │
                    ▼
          ┌──────────────────────┐
          │    AlarmPusher       │
          │ 一级告警: 分层扩展率  │
          │ 二级告警: 频率偏移    │
          │ 工况稳定性检测        │
          │ 阶次跟踪/趋势确认     │
          │ MES推送 + 30min冷却  │
          └───────────┬──────────┘
                      │
                      ▼
          ┌──────────────────────┐
          │     InfluxDB 1.8     │
          │ 时序数据库           │
          │ 保留策略/降采样       │
          │ 连接池/批量写入       │
          └──────────────────────┘
```

### 模块化数据流

```
工业以太网传感器
       │
       ▼
HTTP POST /api/v1/sensor/*
       │
       ├─ /strain ──► EthernetDriver ─► StrainInterpolator ─► InfluxDB
       │                (小波去噪)          (克里金插值)
       │
       ├─ /ae      ─► EthernetDriver ─► DamageClassifier ──► DamageFeatures
       │                (工况归一化)        (随机森林)               │
       │                                                               ▼
       └─ /damage  ─────────────────────────────────────────► DamageForwarder
                                                                       │
                          ┌────────────────────────────────────────────┼────────────────────────────────────┐
                          │                                            │                                    │
                          ▼                                            ▼                                    ▼
              写入 InfluxDB                                    AlarmEngine 检查                     AlarmPusher
            (批量/连接池)                               (频率偏移/分层速率)                       (MES推送/冷却)
```

---

## 技术栈

| 层级 | 技术 | 版本 | 说明 |
|------|------|------|------|
| **后端语言** | Rust | 1.77+ | 高性能异步运行时 |
| **Web框架** | Axum | 0.7 | 异步Web框架 |
| **异步运行时** | Tokio | 1.35 | 全功能异步运行时 |
| **机器学习** | Linfa | 0.7 | 随机森林分类 |
| **时序数据库** | InfluxDB | 1.8.10 | 时序数据存储与查询 |
| **监控** | Prometheus | 2.52 | 指标采集 |
| **可视化** | Grafana | 11.1 | 指标仪表盘 |
| **日志** | Tracing | 0.1 | 结构化日志 |
| **前端3D** | Three.js | r128 | WebGL三维渲染 |
| **前端图表** | Chart.js | 4.4 | 图表可视化 |
| **容器** | Docker + Compose | 20.10+ | 容器编排 |

---

## 快速开始

### Docker 一键部署

#### 前置要求
- Docker Engine >= 20.10.0
- Docker Compose >= 2.0.0

#### 启动服务

```bash
# 克隆项目
git clone <repository-url>
cd AI_solo_coder_task_A_036

# 复制环境变量配置
cp .env.example .env

# 一键启动所有服务
docker-compose up -d

# 查看服务状态
docker-compose ps

# 查看日志
docker-compose logs -f backend
docker-compose logs -f simulator
```

#### 访问服务

| 服务 | 地址 | 说明 |
|------|------|------|
| 后端API | http://localhost:8000 | Rust后端服务 |
| 前端界面 | http://localhost:8000/static/index.html | 监控面板 |
| Swagger文档 | http://localhost:8000/swagger-ui | API文档 |
| 健康检查 | http://localhost:8000/health | 服务健康状态 |
| Prometheus指标 | http://localhost:8000/metrics | 系统指标 |
| Prometheus UI | http://localhost:9090 | 指标查询 |
| Grafana | http://localhost:3000 | 仪表盘 (admin/admin123) |
| InfluxDB | http://localhost:8086 | 时序数据库 |

#### 停止服务

```bash
# 停止并保留数据
docker-compose down

# 停止并清除所有数据
docker-compose down -v
```

#### 仅启动部分服务

```bash
# 仅启动数据库和后端
docker-compose up -d influxdb backend

# 启动后端+模拟器（不启动监控）
docker-compose up -d influxdb backend simulator
```

---

### 本地开发环境

#### 1. 启动InfluxDB

```bash
# 方式1: Docker启动
docker run -d \
  --name wind-monitor-influxdb \
  -p 8086:8086 \
  -e INFLUXDB_DB=wind_monitor \
  -e INFLUXDB_ADMIN_USER=admin \
  -e INFLUXDB_ADMIN_PASSWORD=admin123 \
  -v $(pwd)/influxdb/init.iql:/docker-entrypoint-initdb.d/init.iql:ro \
  influxdb:1.8.10-alpine

# 方式2: 本地安装后启动
influxd -config /etc/influxdb/influxdb.conf
```

#### 2. 编译并运行Rust后端

```bash
cd backend

# 设置环境变量
export INFLUXDB_HOST=localhost
export INFLUXDB_PORT=8086
export INFLUXDB_DATABASE=wind_monitor
export INFLUXDB_USER=wind_user
export INFLUXDB_PASSWORD=wind_pass123

# 编译
cargo build --release

# 运行
./target/release/wind_turbine_blade_monitor
```

#### 3. 运行传感器模拟器

```bash
cd simulator

# 安装依赖
pip install -r requirements.txt

# 启动模拟器（100台风机，10分钟间隔）
python sensor_simulator.py

# 启动并启用损伤注入
python sensor_simulator.py \
  --enable-damage-injection \
  --injection-type delamination \
  --injection-target WT050 \
  --injection-interval 300

# 快速测试（10台风机，1分钟间隔）
python sensor_simulator.py -c 10 -i 60
```

#### 4. 访问前端

直接用浏览器打开 `frontend/index.html`，或使用本地静态服务器：

```bash
cd frontend
python -m http.server 8080
# 访问 http://localhost:8080
```

---

## 传感器模拟器

### 配置参数

所有参数可通过环境变量或命令行参数配置，命令行参数优先级更高。

| 环境变量 | 命令行 | 默认值 | 说明 |
|----------|--------|--------|------|
| `API_URL` | `-a, --api-url` | `http://localhost:8000/api/v1/sensor` | 后端API地址 |
| `TURBINE_COUNT` | `-c, --turbine-count` | `100` | 风机数量 |
| `BLADES_PER_TURBINE` | - | `3` | 每台风机叶片数 |
| `STRAIN_SENSORS_PER_BLADE` | `--strain-sensors` | `20` | 每叶片应变传感器数 |
| `AE_SENSORS_PER_BLADE` | `--ae-sensors` | `10` | 每叶片声发射传感器数 |
| `REPORT_INTERVAL` | `-i, --interval` | `600` | 上报间隔（秒） |
| `DAMAGE_PROBABILITY` | - | `0.15` | 自然损伤概率 |
| `DAMAGE_INJECTION_ENABLED` | `--enable-damage-injection` | `false` | 启用自动损伤注入 |
| `DAMAGE_INJECTION_INTERVAL` | `--injection-interval` | `300` | 损伤注入间隔（秒） |
| `INJECTED_DAMAGE_TYPE` | `--injection-type` | `delamination` | 注入损伤类型 |
| `INJECTED_TURBINE_ID` | `--injection-target` | `WT050` | 损伤注入目标风机 |
| `WIND_SPEED_MIN` | - | `5.0` | 最小风速（m/s） |
| `WIND_SPEED_MAX` | - | `20.0` | 最大风速（m/s） |
| `ROTOR_SPEED_MIN` | - | `8.0` | 最小转速（rpm） |
| `ROTOR_SPEED_MAX` | - | `15.0` | 最大转速（rpm） |
| `REQUEST_TIMEOUT` | - | `30` | 请求超时（秒） |
| `MAX_RETRIES` | - | `3` | 最大重试次数 |

### 损伤类型

| 类型 | 说明 | 声发射特征 |
|------|------|------------|
| `matrix` | 基体开裂 | 中频(180-250kHz)，中低幅值 |
| `fiber` | 纤维断裂 | 高频(250-350kHz)，高幅值 |
| `delamination` | 分层损伤 | 低频(80-150kHz)，长持续时间 |

### 交互命令

模拟器运行时支持以下交互命令（输入后按回车）：

```
status [turbine_id]     查看全场或单台风机状态
inject <turbine_id> <blade_id> <damage_type> [severity] [duration]
                         手动注入损伤
clear <turbine_id> <blade_id>  清除指定叶片的注入损伤
list                     列出所有风机健康度
quit                     退出模拟器
```

### 损伤注入示例

```bash
# 查看WT050状态
status WT050

# 向WT050叶片A注入0.6严重度的分层损伤（永久）
inject WT050 A delamination 0.6

# 向WT001叶片B注入0.8严重度的纤维断裂，持续3600秒
inject WT001 B fiber 0.8 3600

# 清除WT050叶片A的损伤
clear WT050 A
```

### 数据量估算

默认配置（100台风机，10分钟间隔）：

- 每轮应变数据点：`100 × 3 × 20 = 6,000` 点
- 每轮声发射事件：约 `100 × 3 × 5 = 1,500` 次
- 每轮损伤特征：`100 × 3 × 3 = 900` 条
- 每天数据量：`(6,000 + 1,500 + 900) × 144 = 1,209,600` 点
- 每天存储占用：约 50-100 MB

---

## API 接口

完整的API文档请访问 Swagger UI: http://localhost:8000/swagger-ui

### 传感器数据接收

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/v1/sensor/strain` | 批量上报应变数据 |
| `POST` | `/api/v1/sensor/ae` | 单条上报声发射事件 |
| `POST` | `/api/v1/sensor/damage` | 批量上报损伤特征 |

### 数据查询

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/api/v1/blade/health` | 查询单叶片健康度 |
| `GET` | `/api/v1/blade/all-health` | 查询所有叶片健康度 |
| `GET` | `/api/v1/blade/strain-history` | 查询应变历史 |
| `GET` | `/api/v1/blade/ae-events` | 查询声发射事件 |
| `GET` | `/api/v1/statistics/damage` | 损伤统计 |
| `GET` | `/api/v1/statistics/health-ranking` | 健康度排行 |
| `GET` | `/api/v1/alarms` | 查询告警列表 |

### 告警管理

| 方法 | 路径 | 说明 |
|------|------|------|
| `POST` | `/api/v1/alarms/:id/acknowledge` | 确认告警 |

### 系统接口

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/health` | 健康检查 |
| `GET` | `/metrics` | Prometheus指标 |

---

## 监控指标

系统通过 Prometheus 暴露以下核心指标，可在 `/metrics` 端点查看：

### HTTP 请求指标

| 指标 | 类型 | 标签 | 说明 |
|------|------|------|------|
| `http_requests_total` | Counter | method, path, status | HTTP请求总数 |
| `http_request_duration_seconds` | Histogram | method, path | HTTP请求延迟分布 |

### 业务指标

| 指标 | 类型 | 说明 |
|------|------|------|
| `sensor_data_received_total` | Counter | 接收的传感器数据批次总数 |
| `sensor_strain_points_total` | Counter | 接收的应变数据点总数 |
| `sensor_ae_events_total` | Counter | 接收的声发射事件总数 |
| `sensor_damage_features_total` | Counter | 接收的损伤特征批次总数 |
| `damage_classifications_total` | Counter | 执行的损伤分类总数 |
| `damage_alerts_triggered_total` | Counter | 触发的告警总数 |
| `strain_interpolations_total` | Counter | 执行的应变插值总数 |

### 集成指标

| 指标 | 类型 | 说明 |
|------|------|------|
| `mes_notifications_sent_total` | Counter | 成功发送的MES通知数 |
| `mes_notifications_failed_total` | Counter | 失败的MES通知数 |
| `influxdb_write_points_total` | Counter | 成功写入InfluxDB的点数 |
| `influxdb_write_failed_total` | Counter | 失败的InfluxDB写入数 |

### 系统状态

| 指标 | 类型 | 说明 |
|------|------|------|
| `ethernet_driver_queue_size` | Gauge | 以太网驱动通道队列大小 |
| `damage_classifier_queue_size` | Gauge | 损伤分类器通道队列大小 |
| `strain_interpolator_queue_size` | Gauge | 应变插值器通道队列大小 |
| `alarm_pusher_queue_size` | Gauge | 告警推送器通道队列大小 |
| `active_turbines` | Gauge | 活跃上报的风机数量 |
| `system_health_score` | Gauge | 系统整体健康度 (0-100) |

### Grafana 仪表盘

docker-compose 已包含 Grafana 服务，访问 http://localhost:3000

- 默认用户名: `admin`
- 默认密码: `admin123`
- 自动配置了 Prometheus 和 InfluxDB 数据源

---

## 数据存储

### 保留策略 (Retention Policies)

| 策略 | 保留时长 | Shard周期 | 用途 |
|------|----------|----------|------|
| `raw_data` | 90天 | 1天 | 原始数据，详细分析 |
| `hourly_agg` | 365天 | 7天 | 小时聚合，趋势分析 |
| `daily_agg` | 1095天 (3年) | 30天 | 日聚合，长期统计 |

### 降采样 (Continuous Queries)

#### 应变数据
- **小时聚合**：mean/max/min/stddev/count，按 turbine_id + blade_id + sensor_id + section 分组
- **日聚合**：mean/max/min/stddev/count，相同分组

#### 声发射数据
- **小时聚合**：count/mean_amp/max_amp/mean_dur/mean_freq/total_energy
- **日聚合**：count/mean_amp/max_amp/mean_dur/total_energy

#### 损伤特征
- **小时聚合**：各损伤概率均值、严重度均值、健康度均值、固有频率均值
- **日聚合**：各损伤概率均值、严重度均值、健康度均值

---

## 配置说明

### 模型配置 (TOML)

配置文件位置: `backend/config/model_config.toml`

```toml
# 随机森林模型参数
[random_forest]
n_trees = 50                    # 决策树数量
max_depth = 10                  # 树最大深度
feature_ranges = {              # 特征归一化范围
  amplitude = [60.0, 100.0],
  duration = [100.0, 5000.0],
  ...
}

# 信号处理参数
[signal_processing]
wavelet_level = 2               # 小波分解层数
threshold_method = "Universal"  # 阈值方法

# 应变插值参数
[strain_interpolation]
grid_resolution = 32            # 插值网格分辨率
variogram_model = "Spherical"   # 变异函数模型
blade_length = 20.0             # 叶片长度(米)
blade_chord = 2.5               # 叶片弦长(米)

# 告警参数
[alarm]
delamination_rate_threshold = 5.0      # 分层扩展速率阈值
frequency_offset_threshold = 10.0      # 频率偏移阈值 (%)
valid_speed_range = [9.0, 15.0]        # 有效转速范围 (rpm)
valid_wind_range = [3.0, 25.0]         # 有效风速范围 (m/s)
trend_min_points = 3                   # 趋势确认最小点数
trend_window_size = 5                  # 趋势确认窗口大小
cooldown_minutes = 30                  # 告警冷却时间 (分钟)

# MES配置
[mes]
api_url = "http://mes.example.com/api/alerts"
api_token = "your_mes_token_here"
enabled = false

# 通道配置
[channels]
strain_buffer_size = 1000
ae_buffer_size = 1000
damage_buffer_size = 500
alarm_buffer_size = 100
```

### 日志配置

| 环境变量 | 值 | 说明 |
|----------|----|------|
| `RUST_LOG` | `info,wind_turbine_blade_monitor=debug` | 日志级别过滤 |
| `LOG_FORMAT` | `pretty` / `json` | 日志输出格式 |

---

## 目录结构

```
AI_solo_coder_task_A_036/
├── backend/                    # Rust后端
│   ├── Cargo.toml             # 项目依赖
│   ├── Dockerfile             # 多阶段构建
│   ├── config/
│   │   └── model_config.toml  # 模型配置
│   └── src/
│       ├── main.rs            # 主程序（tracing+metrics）
│       ├── models/            # 数据模型
│       ├── routes/            # API路由
│       └── services/          # 业务模块
│           ├── ethernet_driver.rs     # 以太网驱动
│           ├── damage_classifier.rs   # 损伤分类器
│           ├── strain_interpolator.rs # 应变插值器
│           ├── alarm_pusher.rs        # 告警推送器
│           ├── signal_processing.rs   # 信号处理
│           ├── influxdb_service.rs    # InfluxDB服务
│           └── ...
│
├── frontend/                   # 前端代码
│   ├── index.html
│   ├── css/style.css
│   └── js/
│       ├── main.js             # 应用主控制器
│       ├── api.js              # API客户端
│       ├── blade_3d_viewer.js  # 叶片3D可视化
│       ├── health_dashboard.js # 健康度仪表盘
│       └── charts.js           # 图表管理器
│
├── simulator/                  # 传感器模拟器
│   ├── Dockerfile
│   ├── requirements.txt
│   └── sensor_simulator.py     # 模拟器主程序
│
├── influxdb/                   # InfluxDB初始化
│   └── init.iql                # 数据库+保留策略+连续查询
│
├── monitoring/                 # 监控配置
│   └── prometheus/
│       └── prometheus.yml      # Prometheus配置
│
├── docker-compose.yml          # 容器编排
├── .env.example                # 环境变量示例
└── README.md                   # 本文档
```

---

## 故障排查

### 常见问题

**Q: 后端启动失败，提示InfluxDB连接错误**
- 检查InfluxDB是否启动：`docker-compose ps influxdb`
- 检查环境变量 `INFLUXDB_HOST` 是否正确
- 检查网络连接：`docker-compose exec backend ping influxdb`

**Q: 模拟器上报成功率低**
- 检查后端是否正常：`curl http://localhost:8000/health`
- 检查网络延迟：`docker-compose exec simulator ping backend`
- 调整 `REQUEST_TIMEOUT` 和 `MAX_RETRIES` 参数

**Q: Prometheus 无数据**
- 检查指标端点：`curl http://localhost:8000/metrics`
- 检查Prometheus配置：`docker-compose exec prometheus cat /etc/prometheus/prometheus.yml`
- 检查Prometheus目标状态：http://localhost:9090/targets

**Q: 前端页面无法加载**
- 检查静态文件服务：`curl http://localhost:8000/static/index.html`
- 检查浏览器控制台报错
- 确认 `backend/src/main.rs` 中的 `nest_service("/static", ...)` 路径正确

### 日志查看

```bash
# 查看后端日志 (JSON格式)
docker-compose logs -f backend

# 查看模拟器日志
docker-compose logs -f simulator --tail=100

# 查看InfluxDB日志
docker-compose logs -f influxdb

# 查看Prometheus日志
docker-compose logs -f prometheus
```

---

## 性能优化建议

### 高并发场景
1. 增加 `CHANNEL_BUFFER_SIZE` 到 2000+
2. 调整 `INFLUXDB_BATCH_SIZE` 到 1000
3. 增加 `INFLUXDB_CONNECTION_POOL_SIZE` 到 16
4. 启用 InfluxDB 数据压缩

### 大规模风机场景 (100+台)
1. 部署独立的 InfluxDB 集群
2. 增加 Kafka 作为消息队列中间层
3. 后端服务水平扩展，使用负载均衡
4. 考虑使用 InfluxDB 2.x 或 TimescaleDB

### 监控告警
1. 配置 Prometheus Alertmanager
2. 设置关键指标告警（队列积压、写入失败率、服务不可用）
3. 对接企业IM/邮件告警

---

## License

Proprietary - 内部使用
