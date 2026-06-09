#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
风力发电机组叶片传感器模拟器
支持100台风机、每台叶片20应变传感器/10声发射传感器、10分钟上报间隔
可注入不同损伤类型的声发射信号用于测试
"""

import os
import sys
import time
import json
import random
import math
import requests
import threading
import argparse
import signal
from datetime import datetime, timezone
from dataclasses import dataclass, asdict, field
from typing import List, Dict, Tuple, Optional
from collections import defaultdict


def get_config(name: str, default=None, cast_type=None):
    """Get configuration from environment variable with fallback to default."""
    value = os.environ.get(name, default)
    if cast_type and value is not None:
        if cast_type == bool and isinstance(value, str):
            return value.lower() in ('true', '1', 'yes')
        try:
            return cast_type(value)
        except (ValueError, TypeError):
            return default
    return value


CONFIG = {
    "api_url": get_config("API_URL", "http://localhost:8000/api/v1/sensor"),
    "turbine_count": get_config("TURBINE_COUNT", 100, int),
    "blades_per_turbine": get_config("BLADES_PER_TURBINE", 3, int),
    "strain_sensors_per_blade": get_config("STRAIN_SENSORS_PER_BLADE", 20, int),
    "ae_sensors_per_blade": get_config("AE_SENSORS_PER_BLADE", 10, int),
    "report_interval": get_config("REPORT_INTERVAL", 600, int),
    "damage_probability": get_config("DAMAGE_PROBABILITY", 0.15, float),
    "damage_injection_enabled": get_config("DAMAGE_INJECTION_ENABLED", False, bool),
    "damage_injection_interval": get_config("DAMAGE_INJECTION_INTERVAL", 300, int),
    "injected_damage_type": get_config("INJECTED_DAMAGE_TYPE", "delamination"),
    "injected_turbine_id": get_config("INJECTED_TURBINE_ID", "WT050"),
    "wind_speed_min": get_config("WIND_SPEED_MIN", 5.0, float),
    "wind_speed_max": get_config("WIND_SPEED_MAX", 20.0, float),
    "rotor_speed_min": get_config("ROTOR_SPEED_MIN", 8.0, float),
    "rotor_speed_max": get_config("ROTOR_SPEED_MAX", 15.0, float),
    "request_timeout": get_config("REQUEST_TIMEOUT", 30, int),
    "max_retries": get_config("MAX_RETRIES", 3, int),
}

SECTIONS = ["root", "mid", "tip"]
BLADE_IDS = ["A", "B", "C"]
DAMAGE_TYPES = ["matrix", "fiber", "delamination"]


@dataclass
class StrainData:
    turbine_id: str
    blade_id: str
    sensor_id: str
    section: str
    strain_value: float
    temperature: float
    position_x: float
    position_y: float
    position_z: float
    timestamp: str


@dataclass
class AEEvent:
    turbine_id: str
    blade_id: str
    sensor_id: str
    section: str
    amplitude: float
    duration: float
    frequency_peak: float
    frequency_center: float
    energy: float
    counts: int
    rise_time: float
    timestamp: str


@dataclass
class DamageFeatures:
    turbine_id: str
    blade_id: str
    section: str
    matrix_cracking_prob: float
    fiber_breakage_prob: float
    delamination_prob: float
    damage_severity: int
    natural_frequency: float
    delamination_rate: float
    health_score: int
    timestamp: str


@dataclass
class InjectedDamage:
    turbine_id: str
    blade_id: str
    damage_type: str
    severity: float  # 0.0 - 1.0
    start_time: float
    duration: float  # seconds, 0 means permanent


class BladeStatus:
    def __init__(self, turbine_id: str, blade_id: str):
        self.turbine_id = turbine_id
        self.blade_id = blade_id
        self.health_score = 100
        self.damage_type = None
        self.damage_progression = 0.0
        self.baseline_frequency = 12.5
        self.section_health = {"root": 100, "mid": 100, "tip": 100}
        self.injected_damage: Optional[InjectedDamage] = None
        self.strain_baselines = {}

    def update_health(self, time_elapsed: float, wind_speed: float, rotor_speed: float):
        damage_factor = 1.0

        if random.random() < CONFIG["damage_probability"] * (time_elapsed / 600):
            self.damage_progression += random.uniform(0.05, 0.25) * (wind_speed / 12)
            damage_factor = max(0.7, 1.0 - self.damage_progression * 0.008)

        if self.injected_damage:
            inject_factor = 1.0 - self.injected_damage.severity * 0.3
            damage_factor = min(damage_factor, inject_factor)
            self.damage_type = self.injected_damage.damage_type

        self.health_score = max(0, min(100, int(
            self.health_score * damage_factor + random.uniform(-1.5, 1.5)
        )))

        for section in SECTIONS:
            self.section_health[section] = max(0, min(100, int(
                self.section_health[section] * damage_factor + random.uniform(-2, 2)
            )))

        if self.damage_progression > 8 and not self.injected_damage:
            if random.random() < 0.25:
                self.damage_type = random.choice(DAMAGE_TYPES)

        if self.injected_damage and self.injected_damage.duration > 0:
            if time.time() - self.injected_damage.start_time > self.injected_damage.duration:
                self.injected_damage = None
                self.damage_type = None

    def inject_damage(self, damage_type: str, severity: float = 0.6, duration: float = 0):
        self.injected_damage = InjectedDamage(
            turbine_id=self.turbine_id,
            blade_id=self.blade_id,
            damage_type=damage_type,
            severity=severity,
            start_time=time.time(),
            duration=duration
        )
        self.damage_type = damage_type
        for section in SECTIONS:
            self.section_health[section] = max(20, int(self.section_health[section] * (1 - severity * 0.5)))
        self.health_score = max(20, int(self.health_score * (1 - severity * 0.4)))

    def clear_injected_damage(self):
        self.injected_damage = None
        self.damage_type = None


class TurbineSimulator:
    def __init__(self, turbine_idx: int):
        self.turbine_id = f"WT{turbine_idx:03d}"
        self.blades = [BladeStatus(self.turbine_id, bid) for bid in BLADE_IDS]
        self.last_report = time.time()
        self.last_damage_injection = 0
        self.current_wind_speed = CONFIG["wind_speed_min"]
        self.current_rotor_speed = CONFIG["rotor_speed_min"]
        self.total_reports = 0
        self.successful_reports = 0
        self._lock = threading.Lock()

    def update_environmental_conditions(self):
        self.current_wind_speed = CONFIG["wind_speed_min"] + \
            (math.sin(time.time() / 3600) + 1) / 2 * (CONFIG["wind_speed_max"] - CONFIG["wind_speed_min"]) + \
            random.uniform(-1, 1)
        self.current_wind_speed = max(
            CONFIG["wind_speed_min"],
            min(CONFIG["wind_speed_max"], self.current_wind_speed)
        )

        self.current_rotor_speed = CONFIG["rotor_speed_min"] + \
            (math.sin(time.time() / 3600 + 0.5) + 1) / 2 * (CONFIG["rotor_speed_max"] - CONFIG["rotor_speed_min"]) + \
            random.uniform(-0.5, 0.5)
        self.current_rotor_speed = max(
            CONFIG["rotor_speed_min"],
            min(CONFIG["rotor_speed_max"], self.current_rotor_speed)
        )

    def generate_strain_data(self, blade: BladeStatus) -> List[StrainData]:
        data = []
        blade_length = 70.0
        wind_factor = (self.current_wind_speed / 12) ** 1.8

        for sensor_idx in range(CONFIG["strain_sensors_per_blade"]):
            position_ratio = (sensor_idx + 1) / (CONFIG["strain_sensors_per_blade"] + 1)
            z_pos = position_ratio * blade_length

            section = SECTIONS[0] if z_pos < 20 else SECTIONS[1] if z_pos < 45 else SECTIONS[2]
            section_health = blade.section_health[section]

            base_strain = 800 + position_ratio * 600
            health_factor = 1.0 + (100 - section_health) * 0.005
            noise = random.gauss(0, 50)
            cycle_strain = math.sin(time.time() * (self.current_rotor_speed / 60) * 2 * math.pi) * 180

            injected_factor = 1.0
            if blade.injected_damage:
                injected_factor = 1.0 + blade.injected_damage.severity * 0.4

            strain_value = (base_strain + cycle_strain + noise) * health_factor * wind_factor * injected_factor

            temp = 20 + math.sin(time.time() / 3600) * 15 + random.gauss(0, 2)

            data.append(StrainData(
                turbine_id=blade.turbine_id,
                blade_id=blade.blade_id,
                sensor_id=f"S{sensor_idx + 1:02d}",
                section=section,
                strain_value=round(strain_value, 2),
                temperature=round(temp, 2),
                position_x=round(random.gauss(0, 0.5), 3),
                position_y=round(random.gauss(1.5, 0.3), 3),
                position_z=round(z_pos, 3),
                timestamp=datetime.now(timezone.utc).isoformat()
            ))

        return data

    def generate_ae_events(self, blade: BladeStatus) -> List[AEEvent]:
        events = []
        baseline_event_count = 2
        health_factor = max(1, (100 - blade.health_score) / 10)
        wind_factor = (self.current_wind_speed / 12) ** 1.2

        injected_boost = 0
        if blade.injected_damage:
            injected_boost = blade.injected_damage.severity * 10

        event_count = int(baseline_event_count + random.poisson((health_factor + injected_boost) * 3) * wind_factor)

        for event_idx in range(event_count):
            sensor_idx = random.randint(0, CONFIG["ae_sensors_per_blade"] - 1)
            position_ratio = (sensor_idx + 1) / (CONFIG["ae_sensors_per_blade"] + 1)
            section = SECTIONS[0] if position_ratio < 0.3 else SECTIONS[1] if position_ratio < 0.7 else SECTIONS[2]
            section_health = blade.section_health[section]

            damage_intensity = max(0, (100 - section_health) / 100)

            damage_type = blade.damage_type
            injected_severity = blade.injected_damage.severity if blade.injected_damage else 0

            if section_health > 80 and not blade.injected_damage:
                amplitude = random.uniform(60, 80)
                duration = random.uniform(200, 800)
                frequency_peak = random.uniform(100, 180)
            elif section_health > 50 and not blade.injected_damage:
                amplitude = random.uniform(75, 95)
                duration = random.uniform(500, 2000)
                frequency_peak = random.uniform(150, 280)
            else:
                amplitude = random.uniform(85, 105)
                duration = random.uniform(1000, 5000)
                frequency_peak = random.uniform(80, 350)

            if damage_type == "matrix":
                amplitude += random.uniform(0, 10 + injected_severity * 10)
                frequency_peak = random.uniform(180, 250)
            elif damage_type == "fiber":
                amplitude += random.uniform(5, 15 + injected_severity * 10)
                frequency_peak = random.uniform(250, 350)
            elif damage_type == "delamination":
                amplitude += random.uniform(10, 20 + injected_severity * 15)
                duration += random.uniform(1000, 3000 + injected_severity * 2000)
                frequency_peak = random.uniform(80, 150)

            if blade.injected_damage and blade.injected_damage.damage_type == damage_type:
                amplitude *= (1 + injected_severity * 0.3)
                duration *= (1 + injected_severity * 0.5)

            frequency_center = frequency_peak + random.uniform(-30, 30)
            energy = amplitude * duration * 0.1
            counts = int(duration / 100 + random.uniform(-5, 10))
            rise_time = duration * random.uniform(0.05, 0.2)

            events.append(AEEvent(
                turbine_id=blade.turbine_id,
                blade_id=blade.blade_id,
                sensor_id=f"AE{sensor_idx + 1:02d}",
                section=section,
                amplitude=round(min(120, amplitude), 2),
                duration=round(max(50, duration), 2),
                frequency_peak=round(max(50, min(400, frequency_peak)), 2),
                frequency_center=round(max(50, min(400, frequency_center)), 2),
                energy=round(max(0, energy), 2),
                counts=max(1, counts),
                rise_time=round(max(10, rise_time), 2),
                timestamp=datetime.now(timezone.utc).isoformat()
            ))

        return events

    def generate_damage_features(self, blade: BladeStatus) -> List[DamageFeatures]:
        features = []

        for section in SECTIONS:
            section_health = blade.section_health[section]
            damage_intensity = max(0, (100 - section_health) / 100)

            injected_severity = blade.injected_damage.severity if blade.injected_damage else 0
            damage_type = blade.damage_type

            if section_health > 80 and not blade.injected_damage:
                matrix_prob = random.uniform(0.0, 0.15)
                fiber_prob = random.uniform(0.0, 0.08)
                delam_prob = random.uniform(0.0, 0.05)
            elif section_health > 50 and not blade.injected_damage:
                matrix_prob = random.uniform(0.1, 0.4)
                fiber_prob = random.uniform(0.05, 0.25)
                delam_prob = random.uniform(0.03, 0.2)
            else:
                prob_base = 0.4 + injected_severity * 0.4
                if damage_type == "matrix":
                    matrix_prob = random.uniform(prob_base, prob_base + 0.4)
                    fiber_prob = random.uniform(0.1, 0.3)
                    delam_prob = random.uniform(0.05, 0.2)
                elif damage_type == "fiber":
                    matrix_prob = random.uniform(0.1, 0.3)
                    fiber_prob = random.uniform(prob_base, prob_base + 0.4)
                    delam_prob = random.uniform(0.05, 0.2)
                elif damage_type == "delamination":
                    matrix_prob = random.uniform(0.1, 0.3)
                    fiber_prob = random.uniform(0.1, 0.3)
                    delam_prob = random.uniform(prob_base, prob_base + 0.4)
                else:
                    matrix_prob = random.uniform(0.2, 0.5)
                    fiber_prob = random.uniform(0.15, 0.4)
                    delam_prob = random.uniform(0.1, 0.35)

            severity = int(damage_intensity * 100)
            frequency_offset = damage_intensity * 15 + injected_severity * 10
            natural_frequency = blade.baseline_frequency * (1 - frequency_offset / 100)
            delamination_rate = delam_prob * 8 + random.uniform(0, 0.5) + injected_severity * 3
            health_score = section_health

            features.append(DamageFeatures(
                turbine_id=blade.turbine_id,
                blade_id=blade.blade_id,
                section=section,
                matrix_cracking_prob=round(matrix_prob, 4),
                fiber_breakage_prob=round(fiber_prob, 4),
                delamination_prob=round(delam_prob, 4),
                damage_severity=severity,
                natural_frequency=round(natural_frequency, 3),
                delamination_rate=round(delamination_rate, 3),
                health_score=health_score,
                timestamp=datetime.now(timezone.utc).isoformat()
            ))

        return features

    def auto_inject_damage(self):
        if not CONFIG["damage_injection_enabled"]:
            return

        time_since_last = time.time() - self.last_damage_injection
        if time_since_last < CONFIG["damage_injection_interval"]:
            return

        if self.turbine_id == CONFIG["injected_turbine_id"]:
            blade = random.choice(self.blades)
            damage_type = CONFIG["injected_damage_type"]
            severity = random.uniform(0.4, 0.8)
            duration = random.choice([0, 1800, 3600])

            blade.inject_damage(damage_type, severity, duration)
            self.last_damage_injection = time.time()

            print(f"[{datetime.now().strftime('%H:%M:%S')}] 💥 注入损伤: {self.turbine_id}-{blade.blade_id} "
                  f"{damage_type} 严重度:{severity:.2f} 持续:{duration}s")

    def report(self) -> bool:
        with self._lock:
            self.update_environmental_conditions()
            self.auto_inject_damage()

            all_strain_data = []
            all_ae_events = []
            all_damage_features = []

            time_elapsed = time.time() - self.last_report

            for blade in self.blades:
                blade.update_health(time_elapsed, self.current_wind_speed, self.current_rotor_speed)
                all_strain_data.extend(self.generate_strain_data(blade))
                all_ae_events.extend(self.generate_ae_events(blade))
                all_damage_features.extend(self.generate_damage_features(blade))

            self.last_report = time.time()
            self.total_reports += 1

        try:
            strain_response = self._send_with_retry(
                f"{CONFIG['api_url']}/strain",
                json={"data": [asdict(d) for d in all_strain_data]},
                params={"wind_speed": round(self.current_wind_speed, 2),
                        "rotor_speed": round(self.current_rotor_speed, 2)}
            )

            ae_success = True
            for ae_event in all_ae_events:
                ae_response = self._send_with_retry(
                    f"{CONFIG['api_url']}/ae",
                    json=asdict(ae_event),
                    params={"wind_speed": round(self.current_wind_speed, 2),
                            "rotor_speed": round(self.current_rotor_speed, 2)}
                )
                if not ae_response or ae_response.status_code != 200:
                    ae_success = False
                    break

            damage_response = self._send_with_retry(
                f"{CONFIG['api_url']}/damage",
                json={"data": [asdict(d) for d in all_damage_features]},
            )

            success = (strain_response and strain_response.status_code == 200 and
                       ae_success and
                       damage_response and damage_response.status_code == 200)

            if success:
                self.successful_reports += 1

            return success

        except Exception as e:
            print(f"[{datetime.now().strftime('%H:%M:%S')}] ❌ {self.turbine_id} 上报异常: {e}")
            return False

    def _send_with_retry(self, url: str, json: dict, params: dict = None) -> Optional[requests.Response]:
        for attempt in range(CONFIG["max_retries"]):
            try:
                response = requests.post(
                    url,
                    json=json,
                    params=params,
                    timeout=CONFIG["request_timeout"]
                )
                return response
            except requests.RequestException:
                if attempt < CONFIG["max_retries"] - 1:
                    time.sleep(1 * (attempt + 1))
        return None

    def manual_inject_damage(self, blade_id: str, damage_type: str, severity: float = 0.6, duration: float = 0):
        for blade in self.blades:
            if blade.blade_id == blade_id:
                blade.inject_damage(damage_type, severity, duration)
                print(f"[{datetime.now().strftime('%H:%M:%S')}] 💉 手动注入: {self.turbine_id}-{blade_id} "
                      f"{damage_type} 严重度:{severity:.2f}")
                return True
        return False

    def get_status(self) -> dict:
        with self._lock:
            return {
                "turbine_id": self.turbine_id,
                "wind_speed": round(self.current_wind_speed, 2),
                "rotor_speed": round(self.current_rotor_speed, 2),
                "health_scores": {b.blade_id: b.health_score for b in self.blades},
                "damage_types": {b.blade_id: b.damage_type for b in self.blades},
                "total_reports": self.total_reports,
                "success_rate": (self.successful_reports / self.total_reports * 100) if self.total_reports > 0 else 100,
            }


class WindFarmSimulator:
    def __init__(self, turbine_count: int = None):
        self.turbine_count = turbine_count or CONFIG["turbine_count"]
        self.turbines = [TurbineSimulator(i + 1) for i in range(self.turbine_count)]
        self.stop_event = threading.Event()
        self.report_count = 0
        self.success_count = 0
        self._lock = threading.Lock()

    def run(self):
        print("=" * 80)
        print("🌬️  风电场叶片传感器模拟器启动")
        print("=" * 80)
        print(f"风机数量: {self.turbine_count} 台")
        print(f"每台叶片数: {CONFIG['blades_per_turbine']}")
        print(f"每台应变传感器: {CONFIG['strain_sensors_per_blade']}")
        print(f"每台声发射传感器: {CONFIG['ae_sensors_per_blade']}")
        print(f"每轮总应变点: {self.turbine_count * CONFIG['blades_per_turbine'] * CONFIG['strain_sensors_per_blade']}")
        print(f"每轮总声发射事件: ~{self.turbine_count * CONFIG['blades_per_turbine'] * 5}")
        print(f"上报间隔: {CONFIG['report_interval']} 秒 ({CONFIG['report_interval']/60:.1f} 分钟)")
        print(f"损伤注入: {'开启' if CONFIG['damage_injection_enabled'] else '关闭'}")
        if CONFIG["damage_injection_enabled"]:
            print(f"目标风机: {CONFIG['injected_turbine_id']}")
            print(f"注入类型: {CONFIG['injected_damage_type']}")
            print(f"注入间隔: {CONFIG['damage_injection_interval']} 秒")
        print(f"API端点: {CONFIG['api_url']}")
        print("=" * 80)
        print()

        signal.signal(signal.SIGINT, self._signal_handler)
        signal.signal(signal.SIGTERM, self._signal_handler)

        self._start_command_listener()

        while not self.stop_event.is_set():
            cycle_start = time.time()

            print(f"\n⏰ [{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] "
                  f"开始第 {self.report_count + 1} 轮上报...")

            threads = []
            for turbine in self.turbines:
                t = threading.Thread(target=self._report_turbine, args=(turbine,), daemon=True)
                t.start()
                threads.append(t)

            for t in threads:
                t.join()

            with self._lock:
                self.report_count += 1
                success_rate = (self.success_count / (self.report_count * self.turbine_count)) * 100

            print(f"\n✅ 第 {self.report_count} 轮上报完成 - "
                  f"成功率: {success_rate:.1f}% "
                  f"({self.success_count}/{self.report_count * self.turbine_count})")

            wait_time = CONFIG["report_interval"] - (time.time() - cycle_start)
            if wait_time > 0 and not self.stop_event.is_set():
                for remaining in range(int(wait_time), 0, -60):
                    if self.stop_event.is_set():
                        break
                    print(f"⏳ 等待 {remaining} 秒后进行下一轮上报...", end="\r")
                    self.stop_event.wait(min(60, remaining))

    def _report_turbine(self, turbine: TurbineSimulator):
        if turbine.report():
            with self._lock:
                self.success_count += 1

    def _signal_handler(self, signum, frame):
        print(f"\n\n📡 收到信号 {signum}，正在停止模拟器...")
        self.stop_event.set()

    def _start_command_listener(self):
        def listener():
            while not self.stop_event.is_set():
                try:
                    command = input().strip()
                    self._handle_command(command)
                except EOFError:
                    break
                except Exception as e:
                    print(f"命令错误: {e}")

        t = threading.Thread(target=listener, daemon=True)
        t.start()
        print("\n💡 支持交互命令 (输入后回车):")
        print("   status [turbine_id] - 查看状态")
        print("   inject <turbine_id> <blade_id> <damage_type> [severity] [duration] - 注入损伤")
        print("   clear <turbine_id> <blade_id> - 清除注入损伤")
        print("   list - 列出所有风机")
        print("   quit - 退出\n")

    def _handle_command(self, command: str):
        parts = command.split()
        if not parts:
            return

        cmd = parts[0].lower()

        if cmd == "status":
            if len(parts) > 1:
                turbine_id = parts[1].upper()
                for t in self.turbines:
                    if t.turbine_id == turbine_id:
                        status = t.get_status()
                        print(f"\n📊 {status['turbine_id']} 状态:")
                        print(f"   风速: {status['wind_speed']} m/s, 转速: {status['rotor_speed']} rpm")
                        for bid in BLADE_IDS:
                            dt = status['damage_types'][bid] or "正常"
                            print(f"   叶片{bid}: 健康度 {status['health_scores'][bid]}, 损伤: {dt}")
                        print(f"   上报: {status['total_reports']} 次, 成功率: {status['success_rate']:.1f}%")
                        return
                print(f"❌ 未找到风机 {turbine_id}")
            else:
                avg_health = sum(min(t.get_status()['health_scores'].values()) for t in self.turbines) / self.turbine_count
                total_reports = sum(t.get_status()['total_reports'] for t in self.turbines)
                print(f"\n📊 全场状态:")
                print(f"   运行风机: {self.turbine_count}")
                print(f"   平均最低健康度: {avg_health:.1f}")
                print(f"   总上报次数: {total_reports}")
                print(f"   当前轮次: {self.report_count}")

        elif cmd == "inject" and len(parts) >= 4:
            turbine_id = parts[1].upper()
            blade_id = parts[2].upper()
            damage_type = parts[3].lower()
            severity = float(parts[4]) if len(parts) > 4 else 0.6
            duration = float(parts[5]) if len(parts) > 5 else 0

            if damage_type not in DAMAGE_TYPES:
                print(f"❌ 无效损伤类型，支持: {', '.join(DAMAGE_TYPES)}")
                return
            if blade_id not in BLADE_IDS:
                print(f"❌ 无效叶片ID，支持: {', '.join(BLADE_IDS)}")
                return

            for t in self.turbines:
                if t.turbine_id == turbine_id:
                    t.manual_inject_damage(blade_id, damage_type, severity, duration)
                    return
            print(f"❌ 未找到风机 {turbine_id}")

        elif cmd == "clear" and len(parts) >= 3:
            turbine_id = parts[1].upper()
            blade_id = parts[2].upper()

            for t in self.turbines:
                if t.turbine_id == turbine_id:
                    for b in t.blades:
                        if b.blade_id == blade_id:
                            b.clear_injected_damage()
                            print(f"✅ 已清除 {turbine_id}-{blade_id} 的注入损伤")
                            return
            print(f"❌ 未找到")

        elif cmd == "list":
            for i in range(0, self.turbine_count, 10):
                batch = self.turbines[i:i+10]
                line = "  ".join(f"{t.turbine_id}[{min(t.get_status()['health_scores'].values())}]" for t in batch)
                print(line)

        elif cmd == "quit":
            self.stop_event.set()

        else:
            print(f"❓ 未知命令: {command}")

    def stop(self):
        print("\n🔴 正在停止模拟器...")
        self.stop_event.set()


def parse_args():
    parser = argparse.ArgumentParser(
        description="风电场叶片传感器模拟器",
        formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("-c", "--turbine-count", type=int,
                        help=f"风机数量 (默认: {CONFIG['turbine_count']})")
    parser.add_argument("-i", "--interval", type=int,
                        help=f"上报间隔秒数 (默认: {CONFIG['report_interval']})")
    parser.add_argument("-a", "--api-url", type=str,
                        help=f"API端点URL (默认: {CONFIG['api_url']})")
    parser.add_argument("--strain-sensors", type=int,
                        help=f"每叶片应变传感器数 (默认: {CONFIG['strain_sensors_per_blade']})")
    parser.add_argument("--ae-sensors", type=int,
                        help=f"每叶片声发射传感器数 (默认: {CONFIG['ae_sensors_per_blade']})")
    parser.add_argument("--enable-damage-injection", action="store_true",
                        help="启用自动损伤注入")
    parser.add_argument("--injection-interval", type=int,
                        help=f"损伤注入间隔秒数 (默认: {CONFIG['damage_injection_interval']})")
    parser.add_argument("--injection-target", type=str,
                        help=f"损伤注入目标风机 (默认: {CONFIG['injected_turbine_id']})")
    parser.add_argument("--injection-type", type=str,
                        choices=DAMAGE_TYPES,
                        help=f"损伤注入类型 (默认: {CONFIG['injected_damage_type']})")
    parser.add_argument("--list-damage-types", action="store_true",
                        help="列出支持的损伤类型并退出")

    return parser.parse_args()


def main():
    args = parse_args()

    if args.list_damage_types:
        print("支持的损伤类型:")
        print("  matrix     - 基体开裂")
        print("  fiber      - 纤维断裂")
        print("  delamination - 分层损伤")
        sys.exit(0)

    if args.turbine_count:
        CONFIG["turbine_count"] = args.turbine_count
    if args.interval:
        CONFIG["report_interval"] = args.interval
    if args.api_url:
        CONFIG["api_url"] = args.api_url
    if args.strain_sensors:
        CONFIG["strain_sensors_per_blade"] = args.strain_sensors
    if args.ae_sensors:
        CONFIG["ae_sensors_per_blade"] = args.ae_sensors
    if args.enable_damage_injection:
        CONFIG["damage_injection_enabled"] = True
    if args.injection_interval:
        CONFIG["damage_injection_interval"] = args.injection_interval
    if args.injection_target:
        CONFIG["injected_turbine_id"] = args.injection_target
    if args.injection_type:
        CONFIG["injected_damage_type"] = args.injection_type

    simulator = WindFarmSimulator()

    try:
        simulator.run()
    except KeyboardInterrupt:
        simulator.stop()
        print("\n✅ 模拟器已停止")
    except Exception as e:
        print(f"\n❌ 模拟器异常退出: {e}")
        raise


if __name__ == "__main__":
    main()
