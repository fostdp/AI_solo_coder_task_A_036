class Application {
    constructor() {
        this.api = window.api || new ApiClient('http://localhost:8000/api/v1');
        this.bladeViewer = null;
        this.healthDashboard = null;
        this.currentView = 'dashboard';
        this.currentTurbine = 'WT001';
        this.currentBlade = 'A';
        this.currentSection = 'mid';
        this.alarms = [];
        this.healthData = [];
    }

    async init() {
        console.log('[Application] 系统初始化开始...');

        this.setupLoadingScreen();
        this.setupNavigation();
        this.setupTurbineSelector();
        this.setupBladeViewer();
        this.setupHealthDashboard();
        this.setupControlPanel();
        this.setupAlarmCenter();
        this.setupCharts();

        this.startClock();
        this.startDataRefresh();

        document.getElementById('loadingScreen').style.display = 'none';

        console.log('[Application] 系统初始化完成');
    }

    setupLoadingScreen() {
        const progress = document.getElementById('loadingProgress');
        const loadingText = document.getElementById('loadingText');
        let progressValue = 0;

        const interval = setInterval(() => {
            progressValue += Math.random() * 15;
            if (progressValue >= 100) {
                progressValue = 100;
                clearInterval(interval);
            }
            if (progress) progress.style.width = `${progressValue}%`;

            if (loadingText) {
                if (progressValue < 30) loadingText.textContent = '初始化Three.js引擎...';
                else if (progressValue < 60) loadingText.textContent = '加载数据模块...';
                else if (progressValue < 90) loadingText.textContent = '连接后端服务...';
                else loadingText.textContent = '系统就绪';
            }
        }, 100);
    }

    setupNavigation() {
        const navItems = document.querySelectorAll('.nav-item');
        const views = document.querySelectorAll('.view-container');

        navItems.forEach(item => {
            item.addEventListener('click', () => {
                const view = item.dataset.view;

                navItems.forEach(nav => nav.classList.remove('active'));
                item.classList.add('active');

                views.forEach(v => v.style.display = 'none');
                document.getElementById(`${view}View`).style.display = 'block';

                this.currentView = view;
                console.log(`[Application] 切换视图: ${view}`);

                if (view === 'blade3d' && this.bladeViewer) {
                    this.bladeViewer.loadBladeData();
                } else if (view === 'health' && this.healthDashboard) {
                    this.healthDashboard.refresh();
                } else if (view === 'alarms') {
                    this.loadAlarms();
                } else if (view === 'dashboard') {
                    this.loadDashboardData();
                }
            });
        });
    }

    setupTurbineSelector() {
        const turbineSelect = document.getElementById('turbineSelect');
        const bladeButtons = document.querySelectorAll('.blade-btn');

        if (turbineSelect) {
            for (let i = 1; i <= 100; i++) {
                const option = document.createElement('option');
                option.value = `WT${String(i).padStart(3, '0')}`;
                option.textContent = `风机 ${String(i).padStart(3, '0')}`;
                turbineSelect.appendChild(option);
            }

            turbineSelect.addEventListener('change', (e) => {
                this.currentTurbine = e.target.value;
                if (this.bladeViewer) {
                    this.bladeViewer.setCurrentBlade(this.currentTurbine, this.currentBlade);
                }
                this.loadBladeInfo();
            });
        }

        bladeButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                bladeButtons.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.currentBlade = btn.dataset.blade;
                if (this.bladeViewer) {
                    this.bladeViewer.setCurrentBlade(this.currentTurbine, this.currentBlade);
                }
                this.loadBladeInfo();
            });
        });
    }

    setupBladeViewer() {
        if (typeof Blade3DViewer !== 'undefined') {
            this.bladeViewer = new Blade3DViewer('threeContainer', this.api);
            this.bladeViewer.onSectionClick = (section, turbine, blade) => {
                this.currentSection = section;
                this.bladeViewer.setCurrentSection(section);
                this.showSectionDetail(section, turbine, blade);
            };
            this.bladeViewer.init();
        } else {
            console.warn('[Application] Blade3DViewer 未定义');
        }

        document.getElementById('closeDetail')?.addEventListener('click', () => {
            document.getElementById('sectionDetailPanel').style.display = 'none';
        });
    }

    setupHealthDashboard() {
        if (typeof HealthDashboard !== 'undefined') {
            this.healthDashboard = new HealthDashboard('healthView', this.api, {
                onTurbineSelect: (turbineId) => {
                    this.currentTurbine = turbineId;
                    document.getElementById('turbineSelect').value = turbineId;
                    document.querySelector('.nav-item[data-view="blade3d"]').click();
                    if (this.bladeViewer) {
                        this.bladeViewer.setCurrentBlade(turbineId, this.currentBlade);
                    }
                }
            });
            this.healthDashboard.init();
        } else {
            console.warn('[Application] HealthDashboard 未定义');
        }
    }

    setupControlPanel() {
        const modeButtons = document.querySelectorAll('.ctrl-btn');
        const sectionButtons = document.querySelectorAll('.section-btn');
        const heatmapRange = document.getElementById('heatmapRange');
        const maxStrain = document.getElementById('maxStrain');

        modeButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                modeButtons.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                if (this.bladeViewer) {
                    this.bladeViewer.setDisplayMode(btn.dataset.mode);
                }
            });
        });

        sectionButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                sectionButtons.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.currentSection = btn.dataset.section;
                if (this.bladeViewer) {
                    this.bladeViewer.setCurrentSection(btn.dataset.section);
                }
                this.showSectionDetail(btn.dataset.section, this.currentTurbine, this.currentBlade);
            });
        });

        if (heatmapRange && maxStrain) {
            heatmapRange.addEventListener('input', (e) => {
                const value = e.target.value;
                maxStrain.textContent = `${value} με`;
                if (this.bladeViewer) {
                    this.bladeViewer.setHeatmapRange(parseFloat(value));
                }
            });
        }
    }

    setupAlarmCenter() {
        const filterButtons = document.querySelectorAll('.filter-btn');

        filterButtons.forEach(btn => {
            btn.addEventListener('click', () => {
                filterButtons.forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.filterAlarms(btn.dataset.filter);
            });
        });

        this.loadAlarms();
    }

    setupCharts() {
        this.initDamageChart();
        this.initHealthTrendChart();
        this.initDamageTypeChart();
        this.initSeverityChart();
        this.loadDashboardData();
    }

    initDamageChart() {
        const ctx = document.getElementById('damageChart');
        if (!ctx || typeof Chart === 'undefined') return;

        this.damageChart = new Chart(ctx, {
            type: 'doughnut',
            data: {
                labels: ['基体开裂', '纤维断裂', '分层损伤', '无损伤'],
                datasets: [{
                    data: [8, 3, 7, 282],
                    backgroundColor: ['#fbbf24', '#f97316', '#ef4444', '#10b981']
                }]
            },
            options: {
                responsive: true,
                plugins: { legend: { position: 'bottom' } }
            }
        });
    }

    initHealthTrendChart() {
        const ctx = document.getElementById('healthTrendChart');
        if (!ctx || typeof Chart === 'undefined') return;

        const labels = Array.from({ length: 7 }, (_, i) => {
            const d = new Date();
            d.setDate(d.getDate() - 6 + i);
            return `${d.getMonth() + 1}/${d.getDate()}`;
        });

        this.healthTrendChart = new Chart(ctx, {
            type: 'line',
            data: {
                labels,
                datasets: [{
                    label: '平均健康度',
                    data: labels.map(() => 80 + Math.random() * 15),
                    borderColor: '#00d4ff',
                    backgroundColor: 'rgba(0, 212, 255, 0.1)',
                    fill: true,
                    tension: 0.4
                }]
            },
            options: {
                responsive: true,
                scales: { y: { beginAtZero: false, min: 70, max: 100 } }
            }
        });
    }

    initDamageTypeChart() {
        const ctx = document.getElementById('damageTypeChart');
        if (!ctx || typeof Chart === 'undefined') return;

        this.damageTypeChart = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: ['基体开裂', '纤维断裂', '分层损伤'],
                datasets: [{
                    label: '叶片数量',
                    data: [8, 3, 7],
                    backgroundColor: ['#fbbf24', '#f97316', '#ef4444']
                }]
            },
            options: { responsive: true }
        });
    }

    initSeverityChart() {
        const ctx = document.getElementById('severityChart');
        if (!ctx || typeof Chart === 'undefined') return;

        this.severityChart = new Chart(ctx, {
            type: 'pie',
            data: {
                labels: ['轻微 (1级)', '中等 (2级)', '严重 (3级)', '危险 (4级)'],
                datasets: [{
                    data: [5, 6, 4, 3],
                    backgroundColor: ['#fbbf24', '#f97316', '#dc2626', '#991b1b']
                }]
            },
            options: {
                responsive: true,
                plugins: { legend: { position: 'right' } }
            }
        });
    }

    async loadDashboardData() {
        try {
            const response = await this.api.getDamageStatistics();
            if (response && response.success) {
                const stats = response.data;

                document.getElementById('avgHealth').textContent = stats.avg_health_score.toFixed(1);
                document.getElementById('healthyCount').textContent = stats.healthy_count;
                document.getElementById('warningCount').textContent = stats.warning_count;
                document.getElementById('damagedCount').textContent = stats.damaged_count;
                document.getElementById('alarmCount').textContent = `告警: ${stats.active_alarms}`;

                if (stats.active_alarms > 0) {
                    document.getElementById('alarmDot').className = 'status-dot warning active';
                }

                if (this.damageChart) {
                    this.damageChart.data.datasets[0].data = [
                        stats.matrix_cracking_count,
                        stats.fiber_breakage_count,
                        stats.delamination_count,
                        stats.healthy_count
                    ];
                    this.damageChart.update();
                }

                this.loadBladeGrid();
            }
        } catch (e) {
            console.warn('[Application] 加载仪表盘数据失败', e);
        }
    }

    async loadBladeGrid() {
        try {
            const response = await this.api.getAllBladesHealth();
            if (response && response.success) {
                this.healthData = response.data;
                this.renderBladeGrid();
            }
        } catch (e) {
            console.warn('[Application] 加载叶片网格失败', e);
        }
    }

    renderBladeGrid() {
        const grid = document.getElementById('bladeGrid');
        if (!grid) return;

        const getColor = (score) => {
            if (score >= 90) return '#10b981';
            if (score >= 80) return '#34d399';
            if (score >= 60) return '#fbbf24';
            if (score >= 40) return '#f97316';
            return '#ef4444';
        };

        const turbineGroups = {};
        this.healthData.forEach(blade => {
            if (!turbineGroups[blade.turbine_id]) {
                turbineGroups[blade.turbine_id] = [];
            }
            turbineGroups[blade.turbine_id].push(blade);
        });

        grid.innerHTML = Object.entries(turbineGroups).slice(0, 50).map(([turbineId, blades]) => {
            const avgHealth = blades.reduce((s, b) => s + b.health_score, 0) / blades.length;
            return `
                <div class="blade-grid-item" title="${turbineId}: ${avgHealth.toFixed(0)}分">
                    <div class="grid-turbine-label">${turbineId}</div>
                    <div class="grid-blades">
                        ${blades.map(b => `
                            <div class="grid-blade" style="background: ${getColor(b.health_score)}"
                                 title="${b.blade_id}: ${b.health_score}分">
                                ${b.blade_id}
                            </div>
                        `).join('')}
                    </div>
                </div>
            `;
        }).join('');
    }

    async loadBladeInfo() {
        const title = document.getElementById('bladeTitle');
        const scoreText = document.getElementById('scoreText');
        const scoreRing = document.getElementById('scoreRing');

        try {
            const response = await this.api.getBladeHealth(this.currentTurbine, this.currentBlade);
            if (response && response.success) {
                const health = response.data;

                if (title) title.textContent = `${this.currentTurbine} - 叶片 ${this.currentBlade}`;
                if (scoreText) scoreText.textContent = health.health_score;
                if (scoreRing) {
                    const circumference = 314;
                    const offset = circumference - (health.health_score / 100) * circumference;
                    scoreRing.style.strokeDashoffset = offset;
                    scoreRing.style.stroke = health.health_score >= 80 ? '#10b981'
                        : health.health_score >= 60 ? '#fbbf24'
                        : '#ef4444';
                }

                document.getElementById('matrixProb').textContent = `${(health.matrix_cracking_prob * 100).toFixed(1)}%`;
                document.getElementById('fiberProb').textContent = `${(health.fiber_breakage_prob * 100).toFixed(1)}%`;
                document.getElementById('delamProb').textContent = `${(health.delamination_prob * 100).toFixed(1)}%`;

                document.getElementById('matrixBar').style.width = `${health.matrix_cracking_prob * 100}%`;
                document.getElementById('fiberBar').style.width = `${health.fiber_breakage_prob * 100}%`;
                document.getElementById('delamBar').style.width = `${health.delamination_prob * 100}%`;
            }
        } catch (e) {
            console.warn('[Application] 加载叶片信息失败', e);
        }
    }

    async showSectionDetail(section, turbine, blade) {
        const panel = document.getElementById('sectionDetailPanel');
        const title = document.getElementById('detailTitle');
        const sectionNames = { root: '叶根', mid: '叶中', tip: '叶尖' };

        if (title) title.textContent = `${sectionNames[section] || section}截面详细数据`;
        if (panel) panel.style.display = 'block';

        try {
            const [strainData, aeData] = await Promise.all([
                this.api.getStrainHistory(turbine, blade, section, 24),
                this.api.getAEEvents(turbine, blade, section, 24)
            ]);

            this.updateSectionCharts(strainData?.data || [], aeData?.data || []);
            this.updateAEStats(aeData?.data || []);
        } catch (e) {
            console.warn('[Application] 加载截面详情失败', e);
        }
    }

    updateSectionCharts(strainData, aeData) {
        const strainCtx = document.getElementById('strainHistoryChart');
        const aeCtx = document.getElementById('aeChart');

        if (strainCtx && typeof Chart !== 'undefined') {
            if (this.strainHistoryChart) this.strainHistoryChart.destroy();

            this.strainHistoryChart = new Chart(strainCtx, {
                type: 'line',
                data: {
                    labels: strainData.map(d => new Date(d.time).toLocaleTimeString()),
                    datasets: [{
                        label: '应变 (με)',
                        data: strainData.map(d => d.value),
                        borderColor: '#00d4ff',
                        backgroundColor: 'rgba(0, 212, 255, 0.1)',
                        fill: true,
                        tension: 0.4
                    }]
                },
                options: { responsive: true, animation: false }
            });
        }

        if (aeCtx && typeof Chart !== 'undefined') {
            if (this.aeChart) this.aeChart.destroy();

            this.aeChart = new Chart(aeCtx, {
                type: 'scatter',
                data: {
                    datasets: [{
                        label: '声发射事件',
                        data: aeData.map(d => ({
                            x: new Date(d.time).getTime(),
                            y: d.amplitude
                        })),
                        backgroundColor: aeData.map(d => d.amplitude > 90 ? '#ef4444' : '#fbbf24')
                    }]
                },
                options: { responsive: true, animation: false }
            });
        }
    }

    updateAEStats(aeData) {
        if (aeData.length === 0) return;

        document.getElementById('aeTotalCount').textContent = aeData.length;
        document.getElementById('aeAvgAmp').textContent =
            `${(aeData.reduce((s, d) => s + d.amplitude, 0) / aeData.length).toFixed(1)} dB`;
        document.getElementById('aeAvgDur').textContent =
            `${(aeData.reduce((s, d) => s + d.duration, 0) / aeData.length).toFixed(0)} μs`;
        document.getElementById('aeAvgFreq').textContent =
            `${(aeData.reduce((s, d) => s + d.frequency, 0) / aeData.length).toFixed(0)} kHz`;
    }

    async loadAlarms() {
        try {
            const response = await this.api.getAlarms();
            if (response && response.success) {
                this.alarms = response.data;
                this.renderAlarms(this.alarms);
            }
        } catch (e) {
            console.warn('[Application] 加载告警失败', e);
        }
    }

    renderAlarms(alarms) {
        const container = document.getElementById('alarmsContainer');
        if (!container) return;

        if (alarms.length === 0) {
            container.innerHTML = '<div class="empty-state">暂无告警</div>';
            return;
        }

        container.innerHTML = alarms.map(alarm => `
            <div class="alarm-item ${alarm.acknowledged ? 'acknowledged' : ''} level-${alarm.alarm_level}">
                <div class="alarm-header">
                    <span class="alarm-badge">${alarm.alarm_level}告警</span>
                    <span class="alarm-time">${new Date(alarm.timestamp).toLocaleString()}</span>
                </div>
                <div class="alarm-content">
                    <div class="alarm-turbine">${alarm.turbine_id} - 叶片 ${alarm.blade_id}</div>
                    <div class="alarm-message">${alarm.message}</div>
                    <div class="alarm-meta">
                        <span>阈值: ${alarm.threshold}</span>
                        <span>当前值: ${alarm.actual_value.toFixed(1)}</span>
                        <span>${alarm.mes_pushed ? '✓ 已推送MES' : '⚠ 未推送MES'}</span>
                    </div>
                </div>
                ${!alarm.acknowledged ? `
                    <button class="ack-btn" onclick="app.acknowledgeAlarm('${alarm.id}')">
                        确认告警
                    </button>
                ` : '<span class="ack-status">已确认</span>'}
            </div>
        `).join('');
    }

    filterAlarms(filter) {
        let filtered = this.alarms;

        if (filter === 'unack') {
            filtered = this.alarms.filter(a => !a.acknowledged);
        } else if (filter === 'level1' || filter === 'level2') {
            filtered = this.alarms.filter(a => a.alarm_level === (filter === 'level1' ? '一级' : '二级'));
        }

        this.renderAlarms(filtered);
    }

    async acknowledgeAlarm(alarmId) {
        try {
            await this.api.acknowledgeAlarm(alarmId);
            const alarm = this.alarms.find(a => a.id === alarmId);
            if (alarm) alarm.acknowledged = 1;
            this.filterAlarms(document.querySelector('.filter-btn.active').dataset.filter);
        } catch (e) {
            console.error('[Application] 确认告警失败', e);
        }
    }

    startClock() {
        const updateTime = () => {
            const el = document.getElementById('currentTime');
            if (el) {
                el.textContent = new Date().toLocaleString('zh-CN', {
                    year: 'numeric',
                    month: '2-digit',
                    day: '2-digit',
                    hour: '2-digit',
                    minute: '2-digit',
                    second: '2-digit'
                });
            }
        };
        updateTime();
        setInterval(updateTime, 1000);
    }

    startDataRefresh() {
        setInterval(() => {
            this.loadDashboardData();
            this.loadAlarms();
            if (this.currentView === 'blade3d') {
                this.loadBladeInfo();
            }
        }, 60000);
    }
}

const app = new Application();

if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', () => app.init());
} else {
    app.init();
}

window.app = app;
