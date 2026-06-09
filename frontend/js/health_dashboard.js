class HealthDashboard {
    constructor(containerId, apiClient, options = {}) {
        this.containerId = containerId;
        this.api = apiClient;
        this.container = null;
        this.options = {
            showRanking: true,
            showStatistics: true,
            showBladeGrid: true,
            autoRefresh: true,
            refreshInterval: 60000,
            sortOrder: 'desc',
            ...options
        };

        this.healthData = [];
        this.rankingData = [];
        this.statistics = null;
        this.refreshTimer = null;
        this.isInitialized = false;

        this.sortAsc = false;
        this.currentFilter = 'all';

        this.healthColors = {
            excellent: '#10b981',
            good: '#34d399',
            fair: '#fbbf24',
            poor: '#f97316',
            critical: '#ef4444'
        };
    }

    init() {
        this.container = document.getElementById(this.containerId);
        if (!this.container || this.isInitialized) return;

        this.render();
        this.loadData();

        if (this.options.autoRefresh) {
            this.startAutoRefresh();
        }

        this.isInitialized = true;
        console.log(`[HealthDashboard] 初始化完成: ${this.containerId}`);
    }

    render() {
        this.container.innerHTML = `
            <div class="health-dashboard">
                ${this.options.showStatistics ? this.renderStatistics() : ''}
                ${this.options.showBladeGrid ? this.renderBladeGrid() : ''}
                ${this.options.showRanking ? this.renderRanking() : ''}
            </div>
        `;

        this.setupEventListeners();
    }

    renderStatistics() {
        return `
            <div class="dashboard-section stats-section">
                <div class="section-header">
                    <h3>全场健康统计</h3>
                    <span class="refresh-indicator" id="refreshIndicator">
                        <span class="pulse-dot"></span>
                        实时更新
                    </span>
                </div>
                <div class="stats-grid" id="statsGrid">
                    <div class="stat-card">
                        <div class="stat-icon health">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M20.84 4.61a5.5 5.5 0 0 0-7.78 0L12 5.67l-1.06-1.06a5.5 5.5 0 0 0-7.78 7.78l1.06 1.06L12 21.23l7.78-7.78 1.06-1.06a5.5 5.5 0 0 0 0-7.78z"/>
                            </svg>
                        </div>
                        <div class="stat-content">
                            <div class="stat-value" id="avgHealthStat">--</div>
                            <div class="stat-label">平均健康度</div>
                        </div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-icon healthy">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/>
                                <polyline points="22 4 12 14.01 9 11.01"/>
                            </svg>
                        </div>
                        <div class="stat-content">
                            <div class="stat-value" id="healthyCountStat">--</div>
                            <div class="stat-label">健康叶片</div>
                        </div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-icon warning">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86"/>
                                <line x1="12" y1="9" x2="12" y2="13"/>
                                <line x1="12" y1="17" x2="12.01" y2="17"/>
                            </svg>
                        </div>
                        <div class="stat-content">
                            <div class="stat-value" id="warningCountStat">--</div>
                            <div class="stat-label">预警叶片</div>
                        </div>
                    </div>
                    <div class="stat-card">
                        <div class="stat-icon danger">
                            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                                <circle cx="12" cy="12" r="10"/>
                                <line x1="15" y1="9" x2="9" y2="15"/>
                                <line x1="9" y1="9" x2="15" y2="15"/>
                            </svg>
                        </div>
                        <div class="stat-content">
                            <div class="stat-value" id="damagedCountStat">--</div>
                            <div class="stat-label">损伤叶片</div>
                        </div>
                    </div>
                </div>
                <div class="stats-detail">
                    <div class="detail-item">
                        <span class="detail-label">基体开裂</span>
                        <span class="detail-value" id="matrixCount">--</span>
                    </div>
                    <div class="detail-item">
                        <span class="detail-label">纤维断裂</span>
                        <span class="detail-value" id="fiberCount">--</span>
                    </div>
                    <div class="detail-item">
                        <span class="detail-label">分层损伤</span>
                        <span class="detail-value" id="delamCount">--</span>
                    </div>
                    <div class="detail-item">
                        <span class="detail-label">活跃告警</span>
                        <span class="detail-value" id="activeAlarms">--</span>
                    </div>
                </div>
            </div>
        `;
    }

    renderBladeGrid() {
        return `
            <div class="dashboard-section grid-section">
                <div class="section-header">
                    <h3>全场叶片健康度分布</h3>
                    <div class="section-actions">
                        <select class="filter-select" id="healthFilter">
                            <option value="all">全部</option>
                            <option value="excellent">优秀 (90-100)</option>
                            <option value="good">良好 (80-89)</option>
                            <option value="fair">一般 (60-79)</option>
                            <option value="poor">较差 (40-59)</option>
                            <option value="critical">危险 (0-39)</option>
                        </select>
                    </div>
                </div>
                <div class="blade-grid-container">
                    <div class="blade-grid" id="bladeGrid">
                    </div>
                </div>
                <div class="grid-legend">
                    <div class="legend-item">
                        <span class="legend-color" style="background: ${this.healthColors.excellent}"></span>
                        <span>优秀 ≥90</span>
                    </div>
                    <div class="legend-item">
                        <span class="legend-color" style="background: ${this.healthColors.good}"></span>
                        <span>良好 80-89</span>
                    </div>
                    <div class="legend-item">
                        <span class="legend-color" style="background: ${this.healthColors.fair}"></span>
                        <span>一般 60-79</span>
                    </div>
                    <div class="legend-item">
                        <span class="legend-color" style="background: ${this.healthColors.poor}"></span>
                        <span>较差 40-59</span>
                    </div>
                    <div class="legend-item">
                        <span class="legend-color" style="background: ${this.healthColors.critical}"></span>
                        <span>危险 <40</span>
                    </div>
                </div>
            </div>
        `;
    }

    renderRanking() {
        return `
            <div class="dashboard-section ranking-section">
                <div class="section-header">
                    <h3>风机健康度排行榜</h3>
                    <div class="section-actions">
                        <button class="sort-btn ${this.sortAsc ? 'active' : ''}" id="sortAscBtn">升序</button>
                        <button class="sort-btn ${!this.sortAsc ? 'active' : ''}" id="sortDescBtn">降序</button>
                    </div>
                </div>
                <div class="ranking-container" id="rankingContainer">
                    <div class="ranking-loading">
                        <div class="spinner"></div>
                        <span>加载中...</span>
                    </div>
                </div>
            </div>
        `;
    }

    setupEventListeners() {
        const sortAscBtn = document.getElementById('sortAscBtn');
        const sortDescBtn = document.getElementById('sortDescBtn');
        const healthFilter = document.getElementById('healthFilter');

        if (sortAscBtn) {
            sortAscBtn.addEventListener('click', () => {
                this.sortAsc = true;
                this.updateRankingUI();
                sortAscBtn.classList.add('active');
                sortDescBtn.classList.remove('active');
            });
        }

        if (sortDescBtn) {
            sortDescBtn.addEventListener('click', () => {
                this.sortAsc = false;
                this.updateRankingUI();
                sortDescBtn.classList.add('active');
                sortAscBtn.classList.remove('active');
            });
        }

        if (healthFilter) {
            healthFilter.addEventListener('change', (e) => {
                this.currentFilter = e.target.value;
                this.updateBladeGridUI();
            });
        }
    }

    async loadData() {
        console.log('[HealthDashboard] 开始加载数据...');

        try {
            const [statsResponse, rankingResponse, healthResponse] = await Promise.all([
                this.api.getDamageStatistics(),
                this.api.getHealthRankings(),
                this.api.getAllBladesHealth()
            ]);

            if (statsResponse && statsResponse.success) {
                this.statistics = statsResponse.data;
                this.updateStatisticsUI();
            }

            if (rankingResponse && rankingResponse.success) {
                this.rankingData = rankingResponse.data;
                this.updateRankingUI();
            }

            if (healthResponse && healthResponse.success) {
                this.healthData = healthResponse.data;
                this.updateBladeGridUI();
            }

            this.updateRefreshIndicator(true);

        } catch (e) {
            console.error('[HealthDashboard] 加载数据失败', e);
            this.updateRefreshIndicator(false);
        }
    }

    updateStatisticsUI() {
        if (!this.statistics) return;

        const avgHealthEl = document.getElementById('avgHealthStat');
        const healthyCountEl = document.getElementById('healthyCountStat');
        const warningCountEl = document.getElementById('warningCountStat');
        const damagedCountEl = document.getElementById('damagedCountStat');
        const matrixCountEl = document.getElementById('matrixCount');
        const fiberCountEl = document.getElementById('fiberCount');
        const delamCountEl = document.getElementById('delamCount');
        const activeAlarmsEl = document.getElementById('activeAlarms');

        if (avgHealthEl) {
            avgHealthEl.textContent = this.statistics.avg_health_score.toFixed(1);
            avgHealthEl.style.color = this.getHealthColor(this.statistics.avg_health_score);
        }
        if (healthyCountEl) healthyCountEl.textContent = this.statistics.healthy_count;
        if (warningCountEl) warningCountEl.textContent = this.statistics.warning_count;
        if (damagedCountEl) damagedCountEl.textContent = this.statistics.damaged_count;
        if (matrixCountEl) matrixCountEl.textContent = this.statistics.matrix_cracking_count;
        if (fiberCountEl) fiberCountEl.textContent = this.statistics.fiber_breakage_count;
        if (delamCountEl) delamCountEl.textContent = this.statistics.delamination_count;
        if (activeAlarmsEl) activeAlarmsEl.textContent = this.statistics.active_alarms;

        console.log('[HealthDashboard] 统计数据已更新');
    }

    updateRankingUI() {
        const container = document.getElementById('rankingContainer');
        if (!container || this.rankingData.length === 0) return;

        const data = [...this.rankingData];
        data.sort((a, b) => this.sortAsc
            ? a.health_score - b.health_score
            : b.health_score - a.health_score
        );

        data.forEach((item, i) => item.display_rank = i + 1);

        container.innerHTML = data.map(item => this.renderRankingItem(item)).join('');

        container.querySelectorAll('.ranking-item').forEach(item => {
            item.addEventListener('click', () => {
                const turbineId = item.dataset.turbine;
                if (this.options.onTurbineSelect) {
                    this.options.onTurbineSelect(turbineId);
                }
            });
        });

        console.log(`[HealthDashboard] 排行榜已更新，共${data.length}台风机`);
    }

    renderRankingItem(item) {
        const healthColor = this.getHealthColor(item.health_score);
        const rankClass = item.display_rank <= 3 ? 'top-rank' : '';
        const rankEmoji = item.display_rank === 1 ? '🥇'
            : item.display_rank === 2 ? '🥈'
            : item.display_rank === 3 ? '🥉'
            : `#${item.display_rank}`;

        return `
            <div class="ranking-item ${rankClass}" data-turbine="${item.turbine_id}">
                <div class="rank-number">${rankEmoji}</div>
                <div class="turbine-info">
                    <div class="turbine-name">${item.turbine_id}</div>
                    <div class="health-bar-container">
                        <div class="health-bar" style="width: ${item.health_score}%; background: ${healthColor}"></div>
                    </div>
                </div>
                <div class="health-score" style="color: ${healthColor}">
                    ${item.health_score}
                </div>
            </div>
        `;
    }

    updateBladeGridUI() {
        const grid = document.getElementById('bladeGrid');
        if (!grid || this.healthData.length === 0) return;

        const filteredData = this.currentFilter === 'all'
            ? this.healthData
            : this.healthData.filter(b => this.filterByHealth(b.health_score, this.currentFilter));

        if (filteredData.length === 0) {
            grid.innerHTML = '<div class="grid-empty">没有符合筛选条件的叶片</div>';
            return;
        }

        const turbineGroups = {};
        filteredData.forEach(blade => {
            if (!turbineGroups[blade.turbine_id]) {
                turbineGroups[blade.turbine_id] = [];
            }
            turbineGroups[blade.turbine_id].push(blade);
        });

        grid.innerHTML = Object.entries(turbineGroups)
            .slice(0, 100)
            .map(([turbineId, blades]) => this.renderTurbineGroup(turbineId, blades))
            .join('');

        console.log(`[HealthDashboard] 叶片网格已更新，显示${filteredData.length}片叶片`);
    }

    renderTurbineGroup(turbineId, blades) {
        const avgHealth = blades.reduce((sum, b) => sum + b.health_score, 0) / blades.length;
        const healthColor = this.getHealthColor(avgHealth);

        return `
            <div class="turbine-group" data-turbine="${turbineId}">
                <div class="turbine-label" style="border-left: 3px solid ${healthColor}">
                    ${turbineId}
                </div>
                <div class="turbine-blades">
                    ${blades.map(blade => this.renderBladeCell(blade)).join('')}
                </div>
            </div>
        `;
    }

    renderBladeCell(blade) {
        const healthColor = this.getHealthColor(blade.health_score);
        const hasDamage = blade.damage_type && blade.damage_type !== 'none';

        return `
            <div class="blade-cell"
                 style="background: ${healthColor}"
                 data-turbine="${blade.turbine_id}"
                 data-blade="${blade.blade_id}"
                 data-health="${blade.health_score}"
                 data-damage="${blade.damage_type || 'none'}"
                 title="${blade.turbine_id}-${blade.blade_id}: ${blade.health_score}分${hasDamage ? ' (' + blade.damage_type + ')' : ''}">
                <span class="blade-label">${blade.blade_id}</span>
                ${hasDamage ? '<span class="damage-dot"></span>' : ''}
            </div>
        `;
    }

    getHealthColor(score) {
        if (score >= 90) return this.healthColors.excellent;
        if (score >= 80) return this.healthColors.good;
        if (score >= 60) return this.healthColors.fair;
        if (score >= 40) return this.healthColors.poor;
        return this.healthColors.critical;
    }

    filterByHealth(score, filter) {
        switch (filter) {
            case 'excellent': return score >= 90;
            case 'good': return score >= 80 && score < 90;
            case 'fair': return score >= 60 && score < 80;
            case 'poor': return score >= 40 && score < 60;
            case 'critical': return score < 40;
            default: return true;
        }
    }

    updateRefreshIndicator(success) {
        const indicator = document.getElementById('refreshIndicator');
        if (!indicator) return;

        indicator.innerHTML = success
            ? '<span class="pulse-dot success"></span> 实时更新'
            : '<span class="pulse-dot error"></span> 更新失败';

        setTimeout(() => {
            indicator.innerHTML = '<span class="pulse-dot"></span> 实时更新';
        }, 3000);
    }

    startAutoRefresh() {
        if (this.refreshTimer) {
            clearInterval(this.refreshTimer);
        }

        this.refreshTimer = setInterval(() => {
            console.log('[HealthDashboard] 自动刷新数据...');
            this.loadData();
        }, this.options.refreshInterval);

        console.log(`[HealthDashboard] 自动刷新已启用，间隔${this.options.refreshInterval}ms`);
    }

    stopAutoRefresh() {
        if (this.refreshTimer) {
            clearInterval(this.refreshTimer);
            this.refreshTimer = null;
            console.log('[HealthDashboard] 自动刷新已停止');
        }
    }

    destroy() {
        this.stopAutoRefresh();
        if (this.container) {
            this.container.innerHTML = '';
        }
        this.isInitialized = false;
        console.log(`[HealthDashboard] 已销毁: ${this.containerId}`);
    }

    refresh() {
        return this.loadData();
    }

    getAverageHealth() {
        if (!this.statistics) return null;
        return this.statistics.avg_health_score;
    }

    getWorstTurbines(count = 10) {
        const data = [...this.rankingData];
        data.sort((a, b) => a.health_score - b.health_score);
        return data.slice(0, count);
    }

    getBestTurbines(count = 10) {
        const data = [...this.rankingData];
        data.sort((a, b) => b.health_score - a.health_score);
        return data.slice(0, count);
    }
}

if (typeof window !== 'undefined') {
    window.HealthDashboard = HealthDashboard;
}
