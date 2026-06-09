const API_BASE_URL = 'http://localhost:8000/api/v1';

class ApiClient {
    constructor(baseUrl) {
        this.baseUrl = baseUrl;
    }

    async request(endpoint, options = {}) {
        const url = `${this.baseUrl}${endpoint}`;
        const defaultOptions = {
            headers: {
                'Content-Type': 'application/json',
            },
        };

        try {
            const response = await fetch(url, { ...defaultOptions, ...options });
            if (!response.ok) {
                throw new Error(`HTTP error! status: ${response.status}`);
            }
            const data = await response.json();
            return data;
        } catch (error) {
            console.error(`API request failed: ${url}`, error);
            return this.getMockData(endpoint);
        }
    }

    getMockData(endpoint) {
        if (endpoint.includes('/statistics/damage')) {
            return {
                success: true,
                data: {
                    total_blades: 300,
                    healthy_count: 245,
                    warning_count: 42,
                    damaged_count: 13,
                    matrix_cracking_count: 8,
                    fiber_breakage_count: 3,
                    delamination_count: 7,
                    avg_health_score: 87.5,
                    active_alarms: 5
                }
            };
        }

        if (endpoint.includes('/blade/all-health')) {
            const blades = [];
            for (let i = 1; i <= 100; i++) {
                for (const blade of ['A', 'B', 'C']) {
                    const score = 70 + Math.floor(Math.random() * 30);
                    blades.push({
                        turbine_id: `WT${String(i).padStart(3, '0')}`,
                        blade_id: blade,
                        health_score: score,
                        damage_type: score > 80 ? 'none' : (score > 60 ? 'matrix' : 'delamination'),
                        severity_level: score > 80 ? 0 : (score > 60 ? 1 : 2),
                        last_check: new Date().toISOString(),
                        root_health: score + 2,
                        mid_health: score,
                        tip_health: score - 1
                    });
                }
            }
            return { success: true, data: blades };
        }

        if (endpoint.includes('/statistics/health-ranking')) {
            const rankings = [];
            for (let i = 1; i <= 100; i++) {
                const score = 75 + Math.floor(Math.random() * 25);
                rankings.push({
                    turbine_id: `WT${String(i).padStart(3, '0')}`,
                    health_score: score,
                    rank: i
                });
            }
            rankings.sort((a, b) => b.health_score - a.health_score);
            rankings.forEach((r, i) => r.rank = i + 1);
            return { success: true, data: rankings };
        }

        if (endpoint.includes('/blade/health')) {
            const score = 75 + Math.floor(Math.random() * 25);
            return {
                success: true,
                data: {
                    turbine_id: 'WT001',
                    blade_id: 'A',
                    health_score: score,
                    damage_type: score > 80 ? 'none' : 'matrix',
                    severity_level: score > 80 ? 0 : 1,
                    last_check: new Date().toISOString(),
                    root_health: score + 2,
                    mid_health: score,
                    tip_health: score - 1
                }
            };
        }

        if (endpoint.includes('/blade/strain-history')) {
            const history = [];
            const now = Date.now();
            for (let i = 24; i >= 0; i--) {
                history.push({
                    time: new Date(now - i * 3600000).toISOString(),
                    value: 800 + Math.random() * 600 + Math.sin(i * 0.5) * 150
                });
            }
            return { success: true, data: history };
        }

        if (endpoint.includes('/blade/ae-events')) {
            const events = [];
            const now = Date.now();
            for (let i = 0; i < 48; i++) {
                events.push({
                    time: new Date(now - (48 - i) * 1800000).toISOString(),
                    amplitude: 70 + Math.random() * 40,
                    duration: 300 + Math.random() * 2000,
                    frequency: 100 + Math.random() * 250
                });
            }
            return { success: true, data: events };
        }

        if (endpoint.includes('/alarms')) {
            return {
                success: true,
                data: [
                    {
                        id: 'ALARM-001',
                        turbine_id: 'WT015',
                        blade_id: 'A',
                        alarm_level: '一级',
                        alarm_type: 'delamination_rate',
                        message: '分层扩展速率超限：WT015-A 叶中 当前 6.2 mm/h，阈值 5.0 mm/h，损伤概率 72%',
                        threshold: 5.0,
                        actual_value: 6.2,
                        timestamp: new Date(Date.now() - 3600000).toISOString(),
                        acknowledged: 0,
                        mes_pushed: 1
                    },
                    {
                        id: 'ALARM-002',
                        turbine_id: 'WT047',
                        blade_id: 'B',
                        alarm_level: '二级',
                        alarm_type: 'frequency_offset',
                        message: '叶片固有频率偏移超限：WT047-B 叶根 当前偏移 12.5%，阈值 10.0%，健康度 68',
                        threshold: 10.0,
                        actual_value: 12.5,
                        timestamp: new Date(Date.now() - 7200000).toISOString(),
                        acknowledged: 0,
                        mes_pushed: 0
                    },
                    {
                        id: 'ALARM-003',
                        turbine_id: 'WT089',
                        blade_id: 'C',
                        alarm_level: '二级',
                        alarm_type: 'frequency_offset',
                        message: '叶片固有频率偏移超限：WT089-C 叶尖 当前偏移 11.3%，阈值 10.0%，健康度 72',
                        threshold: 10.0,
                        actual_value: 11.3,
                        timestamp: new Date(Date.now() - 10800000).toISOString(),
                        acknowledged: 1,
                        mes_pushed: 1
                    }
                ]
            };
        }

        return { success: false, data: null };
    }

    async getDamageStatistics() {
        return this.request('/statistics/damage');
    }

    async getAllBladesHealth() {
        return this.request('/blade/all-health');
    }

    async getBladeHealth(turbineId, bladeId) {
        return this.request(`/blade/health?turbine_id=${turbineId}&blade_id=${bladeId}`);
    }

    async getStrainHistory(turbineId, bladeId, section = 'mid', hours = 24) {
        return this.request(`/blade/strain-history?turbine_id=${turbineId}&blade_id=${bladeId}&section=${section}&hours=${hours}`);
    }

    async getAEEvents(turbineId, bladeId, section = 'mid', hours = 24) {
        return this.request(`/blade/ae-events?turbine_id=${turbineId}&blade_id=${bladeId}&section=${section}&hours=${hours}`);
    }

    async getHealthRankings() {
        return this.request('/statistics/health-ranking');
    }

    async getAlarms(limit = 100, acknowledged = null) {
        let url = `/alarms?limit=${limit}`;
        if (acknowledged !== null) {
            url += `&acknowledged=${acknowledged}`;
        }
        return this.request(url);
    }

    async acknowledgeAlarm(alarmId) {
        return this.request(`/alarms/${alarmId}/acknowledge`, {
            method: 'POST'
        });
    }
}

const api = new ApiClient(API_BASE_URL);
