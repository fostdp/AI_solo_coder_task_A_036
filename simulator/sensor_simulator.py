#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
风力发电机组叶片传感器模拟器
模拟100台风机，每台3个叶片，每个叶片上的光纤应变传感器和声发射传感器
每10分钟上报一次数据
"""

import time
import json
import random
import math
import requests
import threading
from datetime import datetime, timezone
from dataclasses import dataclass, asdict
from typing import List, Dict, Tuple

CONFIG = {
    "api_url": "http://localhost:8000/api/v1/sensor",
    "turbine_count": 100,
    "blades_per_turbine": 3,
    "strain_sensors_per_blade": 20,
    "ae_sensors_per_blade": 10,
    "report_interval": 600,
    "damage_probability": 0.15,
}

SECTIONS = ["root", "mid", "tip"]
BLADE_IDS = ["A", "B", "C"]


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


class BladeStatus:
    def __init__(self, turbine_id: str, blade_id: str):
        self.turbine_id = turbine_id
        self.blade_id = blade_id
        self.health_score = 100
        self.damage_type = None
        self.damage_progression = 0.0
        self.baseline_frequency = 12.5
        self.section_health = {"root": 100, "mid": 100, "tip": 100}

    def update_health(self, time_elapsed: float):
        damage_factor = 1.0
        if random.random() < CONFIG["damage_probability"]:
            self.damage_progression += random.uniform(0.1, 0.5)
            damage_factor = max(0.5, 1.0 - self.damage_progression * 0.01)

        self.health_score = max(0, min(100, int(self.health_score * damage_factor + random.uniform(-2, 2))))

        for section in SECTIONS:
            self.section_health[section] = max(0, min(100, int(self.section_health[section] * damage_factor + random.uniform(-3, 3))))

        if self.damage_progression > 5:
            if random.random() < 0.3:
                self.damage_type = random.choice(["matrix", "fiber", "delamination"])


class TurbineSimulator:
    def __init__(self, turbine_idx: int):
        self.turbine_id = f"WT{turbine_idx:03d}"
        self.blades = [BladeStatus(self.turbine_id, bid) for bid in BLADE_IDS]
        self.last_report = time.time()
        self.ae_events_buffer: List[AEEvent] = []

    def generate_strain_data(self, blade: BladeStatus) -> List[StrainData]:
        data = []
        blade_length = 70.0

        for sensor_idx in range(CONFIG["strain_sensors_per_blade"]):
            position_ratio = (sensor_idx + 1) / (CONFIG["strain_sensors_per_blade"] + 1)
            z_pos = position_ratio * blade_length

            section = SECTIONS[0] if z_pos < 20 else SECTIONS[1] if z_pos < 45 else SECTIONS[2]
            section_health = blade.section_health[section]

            base_strain = 800 + position_ratio * 600
            health_factor = 1.0 + (100 - section_health) * 0.005
            noise = random.gauss(0, 50)
            cycle_strain = math.sin(time.time() * 0.1) * 150

            strain_value = (base_strain + cycle_strain + noise) * health_factor

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
        event_count = int(baseline_event_count + random.poisson(health_factor * 3))

        for event_idx in range(event_count):
            sensor_idx = random.randint(0, CONFIG["ae_sensors_per_blade"] - 1)
            position_ratio = (sensor_idx + 1) / (CONFIG["ae_sensors_per_blade"] + 1)
            section = SECTIONS[0] if position_ratio < 0.3 else SECTIONS[1] if position_ratio < 0.7 else SECTIONS[2]
            section_health = blade.section_health[section]

            damage_intensity = max(0, (100 - section_health) / 100)

            if section_health > 80:
                amplitude = random.uniform(60, 80)
                duration = random.uniform(200, 800)
                frequency_peak = random.uniform(100, 180)
            elif section_health > 50:
                amplitude = random.uniform(75, 95)
                duration = random.uniform(500, 2000)
                frequency_peak = random.uniform(150, 280)
            else:
                amplitude = random.uniform(85, 105)
                duration = random.uniform(1000, 5000)
                frequency_peak = random.uniform(80, 350)

            if blade.damage_type == "matrix":
                amplitude += random.uniform(0, 10)
                frequency_peak = random.uniform(180, 250)
            elif blade.damage_type == "fiber":
                amplitude += random.uniform(5, 15)
                frequency_peak = random.uniform(250, 350)
            elif blade.damage_type == "delamination":
                amplitude += random.uniform(10, 20)
                duration += random.uniform(1000, 3000)
                frequency_peak = random.uniform(80, 150)

            frequency_center = frequency_peak + random.uniform(-30, 30)
            energy = amplitude * duration * 0.1
            counts = int(duration / 100 + random.uniform(-5, 10))
            rise_time = duration * random.uniform(0.05, 0.2)

            events.append(AEEvent(
                turbine_id=blade.turbine_id,
                blade_id=blade.blade_id,
                sensor_id=f"AE{sensor_idx + 1:02d}",
                section=section,
                amplitude=round(amplitude, 2),
                duration=round(duration, 2),
                frequency_peak=round(frequency_peak, 2),
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

            if section_health > 80:
                matrix_prob = random.uniform(0.0, 0.15)
                fiber_prob = random.uniform(0.0, 0.08)
                delam_prob = random.uniform(0.0, 0.05)
            elif section_health > 50:
                matrix_prob = random.uniform(0.1, 0.4)
                fiber_prob = random.uniform(0.05, 0.25)
                delam_prob = random.uniform(0.03, 0.2)
            else:
                if blade.damage_type == "matrix":
                    matrix_prob = random.uniform(0.5, 0.9)
                    fiber_prob = random.uniform(0.1, 0.3)
                    delam_prob = random.uniform(0.05, 0.2)
                elif blade.damage_type == "fiber":
                    matrix_prob = random.uniform(0.1, 0.3)
                    fiber_prob = random.uniform(0.5, 0.9)
                    delam_prob = random.uniform(0.05, 0.2)
                elif blade.damage_type == "delamination":
                    matrix_prob = random.uniform(0.1, 0.3)
                    fiber_prob = random.uniform(0.1, 0.3)
                    delam_prob = random.uniform(0.5, 0.9)
                else:
                    matrix_prob = random.uniform(0.2, 0.5)
                    fiber_prob = random.uniform(0.15, 0.4)
                    delam_prob = random.uniform(0.1, 0.35)

            severity = int(damage_intensity * 100)
            frequency_offset = damage_intensity * 15
            natural_frequency = blade.baseline_frequency * (1 - frequency_offset / 100)
            delamination_rate = delam_prob * 8 + random.uniform(0, 0.5)
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

    def report(self) -> bool:
        all_strain_data = []
        all_ae_events = []
        all_damage_features = []

        time_elapsed = time.time() - self.last_report

        for blade in self.blades:
            blade.update_health(time_elapsed)

            all_strain_data.extend(self.generate_strain_data(blade))
            all_ae_events.extend(self.generate_ae_events(blade))
            all_damage_features.extend(self.generate_damage_features(blade))

        self.last_report = time.time()

        try:
            strain_response = requests.post(
                f"{CONFIG['api_url']}/strain",
                json={"data": [asdict(d) for d in all_strain_data]},
                timeout=30
            )

            ae_response = requests.post(
                f"{CONFIG['api_url']}/ae",
                json={"events": [asdict(e) for e in all_ae_events]},
                timeout=30
            )

            success = strain_response.status_code == 200 and ae_response.status_code == 200

            if success:
                print(f"[{datetime.now().strftime('%H:%M:%S')}] {self.turbine_id} 上报完成 - "
                      f"应变:{len(all_strain_data)} 声发射:{len(all_ae_events)} "
                      f"健康度:{min(b.health_score for b in self.blades)}")
            else:
                print(f"[{datetime.now().strftime('%H:%M:%S')}] {self.turbine_id} 上报失败 - "
                      f"应变:{strain_response.status_code} 声发射:{ae_response.status_code}")

            return success

        except Exception as e:
            print(f"[{datetime.now().strftime('%H:%M:%S')}] {self.turbine_id} 上报异常: {e}")
            return False


class WindFarmSimulator:
    def __init__(self):
        self.turbines = [TurbineSimulator(i + 1) for i in range(CONFIG["turbine_count"])]
        self.stop_event = threading.Event()
        self.report_count = 0
        self.success_count = 0

    def run(self):
        print("=" * 60)
        print("风电场叶片传感器模拟器启动")
        print(f"风机数量: {CONFIG['turbine_count']} 台")
        print(f"每台叶片数: {CONFIG['blades_per_turbine']}")
        print(f"每台应变传感器: {CONFIG['strain_sensors_per_blade']}")
        print(f"每台声发射传感器: {CONFIG['ae_sensors_per_blade']}")
        print(f"上报间隔: {CONFIG['report_interval']} 秒 ({CONFIG['report_interval']/60:.1f} 分钟)")
        print("=" * 60)
        print()

        while not self.stop_event.is_set():
            cycle_start = time.time()

            print(f"\n[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}] "
                  f"开始第 {self.report_count + 1} 轮上报...")

            threads = []
            for turbine in self.turbines:
                t = threading.Thread(target=self._report_turbine, args=(turbine,))
                t.start()
                threads.append(t)

            for t in threads:
                t.join()

            success_rate = (self.success_count / (self.report_count * CONFIG["turbine_count"])) * 100 \
                if self.report_count > 0 else 100

            print(f"\n第 {self.report_count} 轮上报完成 - "
                  f"成功率: {success_rate:.1f}% "
                  f"({self.success_count}/{self.report_count * CONFIG['turbine_count']})")

            wait_time = CONFIG["report_interval"] - (time.time() - cycle_start)
            if wait_time > 0 and not self.stop_event.is_set():
                print(f"等待 {wait_time:.1f} 秒后进行下一轮上报...")
                self.stop_event.wait(wait_time)

    def _report_turbine(self, turbine: TurbineSimulator):
        if turbine.report():
            self.success_count += 1
        self.report_count += 1

    def stop(self):
        print("\n正在停止模拟器...")
        self.stop_event.set()


def main():
    simulator = WindFarmSimulator()

    try:
        simulator.run()
    except KeyboardInterrupt:
        simulator.stop()
        print("\n模拟器已停止")
    except Exception as e:
        print(f"\n模拟器异常退出: {e}")
        raise


if __name__ == "__main__":
    main()
