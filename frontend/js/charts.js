class ChartManager {
    constructor(apiClient) {
        this.api = apiClient;
        this.charts = {};
    }

    initDamageStatisticsChart(canvasId, data) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        this.charts[canvasId] = new Chart(ctx, {
            type: 'doughnut',
            data: {
                labels: ['基体开裂', '纤维断裂', '分层损伤', '健康'],
                datasets: [{
                    data: [
                        data.matrix_cracking_count || 0,
                        data.fiber_breakage_count || 0,
                        data.delamination_count || 0,
                        data.healthy_count || 0
                    ],
                    backgroundColor: ['#fbbf24', '#f97316', '#ef4444', '#10b981'],
                    borderWidth: 0
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                cutout: '60%',
                plugins: {
                    legend: {
                        position: 'bottom',
                        labels: { padding: 20, usePointStyle: true }
                    }
                }
            }
        });

        return this.charts[canvasId];
    }

    initHealthTrendChart(canvasId, data = []) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        const labels = data.length > 0
            ? data.map(d => new Date(d.time).toLocaleDateString())
            : Array.from({ length: 7 }, (_, i) => {
                const d = new Date();
                d.setDate(d.getDate() - 6 + i);
                return `${d.getMonth() + 1}/${d.getDate()}`;
            });

        const values = data.length > 0
            ? data.map(d => d.value)
            : labels.map(() => 80 + Math.random() * 15);

        this.charts[canvasId] = new Chart(ctx, {
            type: 'line',
            data: {
                labels,
                datasets: [{
                    label: '平均健康度',
                    data: values,
                    borderColor: '#00d4ff',
                    backgroundColor: 'rgba(0, 212, 255, 0.1)',
                    fill: true,
                    tension: 0.4,
                    pointRadius: 4,
                    pointBackgroundColor: '#00d4ff'
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                scales: {
                    y: {
                        beginAtZero: false,
                        min: 70,
                        max: 100,
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    },
                    x: {
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    }
                },
                plugins: {
                    legend: { display: false }
                }
            }
        });

        return this.charts[canvasId];
    }

    initDamageTypeBarChart(canvasId, data) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        this.charts[canvasId] = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: ['基体开裂', '纤维断裂', '分层损伤'],
                datasets: [{
                    label: '叶片数量',
                    data: [
                        data.matrix_cracking_count || 0,
                        data.fiber_breakage_count || 0,
                        data.delamination_count || 0
                    ],
                    backgroundColor: [
                        'rgba(251, 191, 36, 0.8)',
                        'rgba(249, 115, 22, 0.8)',
                        'rgba(239, 68, 68, 0.8)'
                    ],
                    borderColor: [
                        '#fbbf24',
                        '#f97316',
                        '#ef4444'
                    ],
                    borderWidth: 2,
                    borderRadius: 8
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                scales: {
                    y: {
                        beginAtZero: true,
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    },
                    x: {
                        grid: { display: false }
                    }
                }
            }
        });

        return this.charts[canvasId];
    }

    initSeverityPieChart(canvasId, counts) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        this.charts[canvasId] = new Chart(ctx, {
            type: 'pie',
            data: {
                labels: ['轻微 (1级)', '中等 (2级)', '严重 (3级)', '危险 (4级)'],
                datasets: [{
                    data: counts || [5, 6, 4, 3],
                    backgroundColor: [
                        'rgba(251, 191, 36, 0.9)',
                        'rgba(249, 115, 22, 0.9)',
                        'rgba(220, 38, 38, 0.9)',
                        'rgba(153, 27, 27, 0.9)'
                    ],
                    borderWidth: 2,
                    borderColor: '#0a0e1a'
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                plugins: {
                    legend: {
                        position: 'right',
                        labels: { padding: 15, usePointStyle: true }
                    }
                }
            }
        });

        return this.charts[canvasId];
    }

    initStrainHistoryChart(canvasId, strainData) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        if (this.charts[canvasId]) {
            this.charts[canvasId].destroy();
        }

        this.charts[canvasId] = new Chart(ctx, {
            type: 'line',
            data: {
                labels: strainData.map(d => new Date(d.time).toLocaleTimeString('zh-CN', {
                    hour: '2-digit',
                    minute: '2-digit'
                })),
                datasets: [{
                    label: '应变 (με)',
                    data: strainData.map(d => d.value),
                    borderColor: '#00d4ff',
                    backgroundColor: 'rgba(0, 212, 255, 0.15)',
                    fill: true,
                    tension: 0.4,
                    pointRadius: 3,
                    pointHoverRadius: 6
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                animation: { duration: 500 },
                scales: {
                    y: {
                        beginAtZero: false,
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    },
                    x: {
                        maxTicksLimit: 8,
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    }
                },
                plugins: {
                    legend: { display: false },
                    tooltip: {
                        callbacks: {
                            label: (ctx) => `应变: ${ctx.raw.toFixed(0)} με`
                        }
                    }
                }
            }
        });

        return this.charts[canvasId];
    }

    initAEChart(canvasId, aeData) {
        const ctx = document.getElementById(canvasId);
        if (!ctx || typeof Chart === 'undefined') return null;

        if (this.charts[canvasId]) {
            this.charts[canvasId].destroy();
        }

        this.charts[canvasId] = new Chart(ctx, {
            type: 'scatter',
            data: {
                datasets: [{
                    label: '声发射事件',
                    data: aeData.map(d => ({
                        x: new Date(d.time).getTime(),
                        y: d.amplitude,
                        r: Math.max(3, d.duration / 500)
                    })),
                    backgroundColor: aeData.map(d =>
                        d.amplitude > 90 ? 'rgba(239, 68, 68, 0.8)'
                            : d.amplitude > 80 ? 'rgba(251, 191, 36, 0.8)'
                            : 'rgba(16, 185, 129, 0.8)'
                    ),
                    borderColor: aeData.map(d =>
                        d.amplitude > 90 ? '#ef4444'
                            : d.amplitude > 80 ? '#fbbf24'
                            : '#10b981'
                    ),
                    borderWidth: 2
                }]
            },
            options: {
                responsive: true,
                maintainAspectRatio: false,
                animation: { duration: 500 },
                scales: {
                    x: {
                        type: 'time',
                        time: {
                            unit: 'hour',
                            displayFormats: { hour: 'HH:mm' }
                        },
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    },
                    y: {
                        title: { display: true, text: '幅值 (dB)' },
                        min: 60,
                        max: 110,
                        grid: { color: 'rgba(255,255,255,0.05)' }
                    }
                },
                plugins: {
                    legend: { display: false },
                    tooltip: {
                        callbacks: {
                            label: (ctx) => [
                                `幅值: ${ctx.parsed.y.toFixed(1)} dB`,
                                `时间: ${new Date(ctx.parsed.x).toLocaleString()}`
                            ]
                        }
                    }
                }
            }
        });

        return this.charts[canvasId];
    }

    updateChart(canvasId, newData) {
        const chart = this.charts[canvasId];
        if (!chart) return;

        if (chart.data.datasets[0].data.length === newData.length) {
            chart.data.datasets[0].data = newData;
        } else {
            chart.data.labels = newData.map((_, i) => i);
            chart.data.datasets[0].data = newData;
        }

        chart.update('none');
    }

    destroyAll() {
        Object.values(this.charts).forEach(chart => {
            if (chart && typeof chart.destroy === 'function') {
                chart.destroy();
            }
        });
        this.charts = {};
    }
}

if (typeof window !== 'undefined') {
    window.ChartManager = ChartManager;
}
