use std::f64::consts::PI;
use ndarray::{Array1, Array2};
use num_traits::{Float, FromPrimitive};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalProcessingError {
    #[error("信号长度不足: {0}")]
    InsufficientLength(String),
    #[error("小波去噪失败: {0}")]
    WaveletDenoiseError(String),
    #[error("工况归一化失败: {0}")]
    NormalizationError(String),
    #[error("阶次跟踪失败: {0}")]
    OrderTrackingError(String),
}

pub struct SignalProcessor {
    wavelet_coeffs: WaveletCoefficients,
    wind_speed_baseline: f64,
    rotor_speed_baseline: f64,
}

struct WaveletCoefficients {
    low_pass: Vec<f64>,
    high_pass: Vec<f64>,
    low_rec: Vec<f64>,
    high_rec: Vec<f64>,
}

impl Default for SignalProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalProcessor {
    pub fn new() -> Self {
        Self {
            wavelet_coeffs: Self::daubechies4(),
            wind_speed_baseline: 8.0,
            rotor_speed_baseline: 12.0,
        }
    }

    fn daubechies4() -> WaveletCoefficients {
        let sqrt2 = (2.0 as f64).sqrt();
        let sqrt3 = (3.0 as f64).sqrt();
        
        let h0 = (1.0 + sqrt3) / (4.0 * sqrt2);
        let h1 = (3.0 + sqrt3) / (4.0 * sqrt2);
        let h2 = (3.0 - sqrt3) / (4.0 * sqrt2);
        let h3 = (1.0 - sqrt3) / (4.0 * sqrt2);
        
        WaveletCoefficients {
            low_pass: vec![h0, h1, h2, h3],
            high_pass: vec![h3, -h2, h1, -h0],
            low_rec: vec![h3, h2, h1, h0],
            high_rec: vec![-h3, h2, -h1, h0],
        }
    }

    pub fn wavelet_denoise(
        &self,
        signal: &[f64],
        level: usize,
        threshold_method: ThresholdMethod,
    ) -> Result<Vec<f64>, SignalProcessingError> {
        if signal.len() < (1 << level) {
            return Err(SignalProcessingError::InsufficientLength(
                format!("信号长度 {} 小于小波分解所需的最小长度 {}", signal.len(), 1 << level)
            ));
        }

        let mut coeffs = self.wavelet_decompose(signal, level)?;

        for i in 1..coeffs.len() {
            let detail = &mut coeffs[i];
            let threshold = self.calculate_threshold(detail, threshold_method);
            for j in 0..detail.len() {
                detail[j] = self.soft_threshold(detail[j], threshold);
            }
        }

        self.wavelet_reconstruct(&coeffs)
    }

    fn wavelet_decompose(
        &self,
        signal: &[f64],
        level: usize,
    ) -> Result<Vec<Vec<f64>>, SignalProcessingError> {
        let mut coeffs = Vec::with_capacity(level + 1);
        let mut current = signal.to_vec();

        for _ in 0..level {
            let (approx, detail) = self.wavelet_transform_step(&current)?;
            coeffs.push(detail);
            current = approx;
        }
        coeffs.push(current);
        coeffs.reverse();

        Ok(coeffs)
    }

    fn wavelet_transform_step(
        &self,
        signal: &[f64],
    ) -> Result<(Vec<f64>, Vec<f64>), SignalProcessingError> {
        let n = signal.len();
        if n < 4 {
            return Err(SignalProcessingError::InsufficientLength(
                "信号长度必须至少为4".to_string()
            ));
        }

        let half = n / 2;
        let mut approx = vec![0.0; half];
        let mut detail = vec![0.0; half];

        for i in 0..half {
            let idx = 2 * i;
            for k in 0..4 {
                let s_idx = (idx + k) % n;
                approx[i] += signal[s_idx] * self.wavelet_coeffs.low_pass[k];
                detail[i] += signal[s_idx] * self.wavelet_coeffs.high_pass[k];
            }
        }

        Ok((approx, detail))
    }

    fn wavelet_reconstruct(
        &self,
        coeffs: &[Vec<f64>],
    ) -> Result<Vec<f64>, SignalProcessingError> {
        if coeffs.is_empty() {
            return Err(SignalProcessingError::InsufficientLength(
                "小波系数为空".to_string()
            ));
        }

        let mut current = coeffs[0].clone();

        for i in 1..coeffs.len() {
            current = self.wavelet_reconstruct_step(&current, &coeffs[i])?;
        }

        Ok(current)
    }

    fn wavelet_reconstruct_step(
        &self,
        approx: &[f64],
        detail: &[f64],
    ) -> Result<Vec<f64>, SignalProcessingError> {
        let n = approx.len();
        let mut reconstructed = vec![0.0; 2 * n];

        for i in 0..n {
            for k in 0..4 {
                let out_idx = (2 * i + k) % (2 * n);
                reconstructed[out_idx] += approx[i] * self.wavelet_coeffs.low_rec[k];
                reconstructed[out_idx] += detail[i] * self.wavelet_coeffs.high_rec[k];
            }
        }

        Ok(reconstructed)
    }

    fn calculate_threshold(&self, detail: &[f64], method: ThresholdMethod) -> f64 {
        match method {
            ThresholdMethod::Universal => {
                let n = detail.len() as f64;
                let sigma = self.median_absolute_deviation(detail) / 0.6745;
                sigma * (2.0 * n.ln()).sqrt()
            }
            ThresholdMethod::SURE => {
                self.sure_threshold(detail)
            }
            ThresholdMethod::Minimax => {
                let n = detail.len() as f64;
                let sigma = self.median_absolute_deviation(detail) / 0.6745;
                sigma * (0.3936 + 0.1829 * n.ln())
            }
        }
    }

    fn median_absolute_deviation(&self, data: &[f64]) -> f64 {
        let median = self.median(data);
        let mut abs_dev: Vec<f64> = data.iter().map(|&x| (x - median).abs()).collect();
        abs_dev.sort_by(|a, b| a.partial_cmp(b).unwrap());
        self.median(&abs_dev)
    }

    fn median(&self, data: &[f64]) -> f64 {
        let mut sorted = data.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let n = sorted.len();
        if n % 2 == 0 {
            (sorted[n/2 - 1] + sorted[n/2]) / 2.0
        } else {
            sorted[n/2]
        }
    }

    fn soft_threshold(&self, x: f64, threshold: f64) -> f64 {
        if x.abs() <= threshold {
            0.0
        } else if x > 0 {
            x - threshold
        } else {
            x + threshold
        }
    }

    fn sure_threshold(&self, detail: &[f64]) -> f64 {
        let n = detail.len() as f64;
        let mut sorted: Vec<f64> = detail.iter().map(|&x| x * x).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mut best_threshold = 0.0;
        let mut min_sure = f64::INFINITY;

        for k in 0..detail.len() {
            let t = sorted[k].sqrt();
            let mut risk = n + 2.0 * k as f64;
            for &s in &sorted[k..] {
                risk += (s.min(t * t) - t * t).max(0.0);
            }
            if risk < min_sure {
                min_sure = risk;
                best_threshold = t;
            }
        }

        best_threshold
    }

    pub fn normalize_by_wind_speed(
        &self,
        signal: f64,
        wind_speed: f64,
        signal_type: SignalType,
    ) -> Result<f64, SignalProcessingError> {
        if wind_speed <= 0.0 {
            return Err(SignalProcessingError::NormalizationError(
                "风速必须大于0".to_string()
            ));
        }

        let wind_ratio = wind_speed / self.wind_speed_baseline;
        
        let correction_factor = match signal_type {
            SignalType::Amplitude => {
                wind_ratio.powf(1.8)
            }
            SignalType::Energy => {
                wind_ratio.powf(3.6)
            }
            SignalType::Frequency => {
                wind_ratio.powf(0.5)
            }
            SignalType::Duration => {
                wind_ratio.powf(-0.3)
            }
        };

        Ok(signal / correction_factor)
    }

    pub fn normalize_ae_features(
        &self,
        amplitude: f64,
        duration: f64,
        frequency_peak: f64,
        energy: f64,
        wind_speed: f64,
        rotor_speed: f64,
    ) -> Result<(f64, f64, f64, f64), SignalProcessingError> {
        let norm_amp = self.normalize_by_wind_speed(amplitude, wind_speed, SignalType::Amplitude)?;
        let norm_dur = self.normalize_by_wind_speed(duration, wind_speed, SignalType::Duration)?;
        let norm_freq = self.normalize_by_wind_speed(frequency_peak, wind_speed, SignalType::Frequency)?;
        let norm_energy = self.normalize_by_wind_speed(energy, wind_speed, SignalType::Energy)?;

        let rotor_ratio = rotor_speed / self.rotor_speed_baseline;
        let rotor_corrected_freq = norm_freq / rotor_ratio.sqrt();

        Ok((norm_amp, norm_dur, rotor_corrected_freq, norm_energy))
    }

    pub fn adaptive_wind_speed_filter(
        &self,
        amplitude: f64,
        wind_speed: f64,
        counts: i32,
    ) -> bool {
        let noise_floor = 55.0 + wind_speed * 1.5;
        let adaptive_threshold = noise_floor + 8.0 * (counts as f64).sqrt();
        
        amplitude > adaptive_threshold
    }

    pub fn denoise_ae_signal(
        &self,
        raw_amplitude: f64,
        raw_duration: f64,
        raw_frequency: f64,
        raw_energy: f64,
        wind_speed: f64,
        rotor_speed: f64,
    ) -> Result<(f64, f64, f64, f64), SignalProcessingError> {
        let mut signal_samples = vec![
            raw_amplitude * 0.8,
            raw_amplitude * 0.95,
            raw_amplitude,
            raw_amplitude * 0.98,
            raw_amplitude * 0.85,
            raw_amplitude * 0.7,
            raw_amplitude * 0.5,
            raw_amplitude * 0.3,
        ];

        let denoised = self.wavelet_denoise(&signal_samples, 2, ThresholdMethod::Universal)?;
        let denoised_amplitude = denoised[2].max(raw_amplitude * 0.6);

        let (norm_amp, norm_dur, norm_freq, norm_energy) = self.normalize_ae_features(
            denoised_amplitude,
            raw_duration,
            raw_frequency,
            raw_energy,
            wind_speed,
            rotor_speed,
        )?;

        Ok((norm_amp, norm_dur, norm_freq, norm_energy))
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ThresholdMethod {
    Universal,
    SURE,
    Minimax,
}

#[derive(Debug, Clone, Copy)]
pub enum SignalType {
    Amplitude,
    Energy,
    Frequency,
    Duration,
}

pub struct OrderTracker {
    sampling_rate: f64,
    order_resolution: f64,
    max_order: f64,
}

impl OrderTracker {
    pub fn new(sampling_rate: f64, max_order: f64, order_resolution: f64) -> Self {
        Self {
            sampling_rate,
            max_order,
            order_resolution,
        }
    }

    pub fn compute_order_spectrum(
        &self,
        signal: &[f64],
        rotor_speed: f64,
        instantaneous_speed: Option<&[f64]>,
    ) -> Result<Array1<f64>, SignalProcessingError> {
        if signal.is_empty() {
            return Err(SignalProcessingError::OrderTrackingError(
                "信号为空".to_string()
            ));
        }

        let n = signal.len();
        let n_fft = n.next_power_of_two();
        let mut padded = signal.to_vec();
        padded.resize(n_fft, 0.0);

        let window = self.hann_window(n_fft);
        for i in 0..n_fft {
            padded[i] *= window[i];
        }

        let spectrum = self.compute_fft(&padded);

        let n_order = (self.max_order / self.order_resolution) as usize;
        let mut order_spectrum = Array1::<f64>::zeros(n_order);

        let fundamental = rotor_speed / 60.0;

        for i in 0..n_order {
            let order = (i as f64 + 0.5) * self.order_resolution;
            let freq = order * fundamental;
            
            let bin_idx = (freq * n_fft as f64 / self.sampling_rate) as usize;
            if bin_idx < spectrum.len() {
                order_spectrum[i] = spectrum[bin_idx];
            }
        }

        Ok(order_spectrum)
    }

    pub fn extract_natural_frequency(
        &self,
        order_spectrum: &Array1<f64>,
        rotor_speed: f64,
    ) -> Result<(f64, f64), SignalProcessingError> {
        let n = order_spectrum.len();
        if n < 3 {
            return Err(SignalProcessingError::OrderTrackingError(
                "阶次谱长度不足".to_string()
            ));
        }

        let mut max_val = 0.0;
        let mut max_idx = 0;

        for i in 2..n-2 {
            let val = order_spectrum[i];
            if val > order_spectrum[i-1] && val > order_spectrum[i+1] && val > max_val {
                let is_valid = val > 1.5 * order_spectrum[i-2] && val > 1.5 * order_spectrum[i+2];
                if is_valid {
                    max_val = val;
                    max_idx = i;
                }
            }
        }

        let order = (max_idx as f64 + 0.5) * self.order_resolution;
        let natural_freq = order * rotor_speed / 60.0;

        let baseline = 12.5;
        let offset = ((natural_freq - baseline) / baseline).abs() * 100.0;

        Ok((natural_freq, offset))
    }

    pub fn normalize_frequency_by_speed(
        &self,
        measured_frequency: f64,
        rotor_speed: f64,
        baseline_speed: f64,
    ) -> f64 {
        let speed_ratio = baseline_speed / rotor_speed;
        measured_frequency * speed_ratio.sqrt()
    }

    pub fn is_frequency_offset_valid(
        &self,
        natural_freq: f64,
        rotor_speed: f64,
        wind_speed: f64,
        threshold: f64,
    ) -> (bool, f64, f64) {
        let baseline = 12.5;
        
        let speed_correction = (self.rotor_speed_baseline() / rotor_speed.max(0.1)).sqrt();
        let wind_correction = 1.0 + (wind_speed - self.wind_speed_baseline()) * 0.005;
        
        let corrected_freq = natural_freq * speed_correction * wind_correction;
        let offset = ((corrected_freq - baseline) / baseline).abs() * 100.0;

        let speed_stable = (rotor_speed - self.rotor_speed_baseline()).abs() < 3.0;
        let wind_stable = wind_speed > 3.0 && wind_speed < 25.0;

        let is_valid = offset > threshold && speed_stable && wind_stable;

        (is_valid, corrected_freq, offset)
    }

    fn hann_window(&self, n: usize) -> Vec<f64> {
        (0..n).map(|i| {
            0.5 * (1.0 - (2.0 * PI * i as f64 / (n - 1) as f64).cos())
        }).collect()
    }

    fn compute_fft(&self, signal: &[f64]) -> Vec<f64> {
        let n = signal.len();
        let mut spectrum = vec![0.0; n / 2 + 1];
        
        for k in 0..=n/2 {
            let mut real = 0.0;
            let mut imag = 0.0;
            
            for t in 0..n {
                let angle = -2.0 * PI * k as f64 * t as f64 / n as f64;
                real += signal[t] * angle.cos();
                imag += signal[t] * angle.sin();
            }
            
            spectrum[k] = (real * real + imag * imag).sqrt() / n as f64;
        }
        
        spectrum
    }

    fn rotor_speed_baseline(&self) -> f64 {
        12.0
    }

    fn wind_speed_baseline(&self) -> f64 {
        8.0
    }
}

pub struct KrigingInterpolator {
    variogram_model: VariogramModel,
    nugget: f64,
    sill: f64,
    range: f64,
}

impl Default for KrigingInterpolator {
    fn default() -> Self {
        Self::new()
    }
}

impl KrigingInterpolator {
    pub fn new() -> Self {
        Self {
            variogram_model: VariogramModel::Spherical,
            nugget: 0.1,
            sill: 1.0,
            range: 5.0,
        }
    }

    pub fn with_params(model: VariogramModel, nugget: f64, sill: f64, range: f64) -> Self {
        Self {
            variogram_model: model,
            nugget,
            sill,
            range,
        }
    }

    pub fn interpolate(
        &self,
        known_points: &[(f64, f64, f64)],
        query_points: &[(f64, f64)],
    ) -> Result<Vec<(f64, f64)>, SignalProcessingError> {
        if known_points.is_empty() {
            return Err(SignalProcessingError::NormalizationError(
                "已知点不能为空".to_string()
            ));
        }

        let n = known_points.len();
        let mut k_matrix = Array2::<f64>::zeros((n + 1, n + 1));

        for i in 0..n {
            for j in 0..n {
                let dist = Self::euclidean_distance(
                    (known_points[i].0, known_points[i].1),
                    (known_points[j].0, known_points[j].1),
                );
                k_matrix[[i, j]] = self.variogram(dist);
            }
            k_matrix[[i, n]] = 1.0;
            k_matrix[[n, i]] = 1.0;
        }
        k_matrix[[n, n]] = 0.0;

        let k_inv = match Self::invert_matrix(&k_matrix) {
            Some(inv) => inv,
            None => {
                return Err(SignalProcessingError::NormalizationError(
                    "克里金矩阵奇异，无法求逆".to_string()
                ));
            }
        };

        let mut results = Vec::with_capacity(query_points.len());

        for &(qx, qy) in query_points {
            let mut k_vector = Array1::<f64>::zeros(n + 1);
            
            for i in 0..n {
                let dist = Self::euclidean_distance(
                    (known_points[i].0, known_points[i].1),
                    (qx, qy),
                );
                k_vector[i] = self.variogram(dist);
            }
            k_vector[n] = 1.0;

            let mut lambda = 0.0;
            let mut weights_sum = 0.0;
            
            for i in 0..n + 1 {
                let mut sum = 0.0;
                for j in 0..n + 1 {
                    sum += k_inv[[i, j]] * k_vector[j];
                }
                if i < n {
                    lambda += sum * known_points[i].2;
                    weights_sum += sum;
                }
            }

            let mut variance = 0.0;
            for i in 0..n + 1 {
                variance += k_vector[i] * (if i < n { k_vector[i] } else { 1.0 });
            }
            for i in 0..n + 1 {
                let mut sum = 0.0;
                for j in 0..n + 1 {
                    sum += k_inv[[i, j]] * k_vector[i] * k_vector[j];
                }
                variance -= sum;
            }
            variance = variance.max(0.0);

            results.push((lambda, variance.sqrt()));
        }

        Ok(results)
    }

    pub fn interpolate_strain_field(
        &self,
        sensor_readings: &[StrainReading],
        grid_resolution: usize,
        blade_length: f64,
        blade_chord: f64,
    ) -> Result<StrainField, SignalProcessingError> {
        let known_points: Vec<(f64, f64, f64)> = sensor_readings
            .iter()
            .map(|r| (r.position_z, r.position_y, r.strain_value))
            .collect();

        let mut query_points = Vec::with_capacity(grid_resolution * grid_resolution);
        for i in 0..grid_resolution {
            let z = (i as f64 / (grid_resolution - 1) as f64 - 0.5) * blade_length;
            for j in 0..grid_resolution {
                let y = (j as f64 / (grid_resolution - 1) as f64 - 0.5) * blade_chord;
                query_points.push((z, y));
            }
        }

        let interpolated = self.interpolate(&known_points, &query_points)?;

        let mut strain_values = vec![0.0; grid_resolution * grid_resolution];
        let mut variances = vec![0.0; grid_resolution * grid_resolution];
        
        for (i, &(val, var)) in interpolated.iter().enumerate() {
            strain_values[i] = val;
            variances[i] = var;
        }

        Ok(StrainField {
            values: strain_values,
            variances,
            resolution: grid_resolution,
            blade_length,
            blade_chord,
        })
    }

    fn variogram(&self, distance: f64) -> f64 {
        let h = distance.abs();
        
        match self.variogram_model {
            VariogramModel::Spherical => {
                if h == 0.0 {
                    0.0
                } else if h <= self.range {
                    self.nugget + (self.sill - self.nugget) * (1.5 * h / self.range - 0.5 * (h / self.range).powi(3))
                } else {
                    self.sill
                }
            }
            VariogramModel::Exponential => {
                self.nugget + (self.sill - self.nugget) * (1.0 - (-h / self.range).exp())
            }
            VariogramModel::Gaussian => {
                self.nugget + (self.sill - self.nugget) * (1.0 - (-h * h / (self.range * self.range)).exp())
            }
            VariogramModel::Linear => {
                self.nugget + (self.sill - self.nugget) * (h / self.range).min(1.0)
            }
        }
    }

    fn euclidean_distance(p1: (f64, f64), p2: (f64, f64)) -> f64 {
        let dx = p1.0 - p2.0;
        let dy = p1.1 - p2.1;
        (dx * dx + dy * dy).sqrt()
    }

    fn invert_matrix(matrix: &Array2<f64>) -> Option<Array2<f64>> {
        let n = matrix.nrows();
        let mut aug = Array2::<f64>::zeros((n, 2 * n));

        for i in 0..n {
            for j in 0..n {
                aug[[i, j]] = matrix[[i, j]];
            }
            aug[[i, n + i]] = 1.0;
        }

        for col in 0..n {
            let mut max_row = col;
            for row in col..n {
                if aug[[row, col]].abs() > aug[[max_row, col]].abs() {
                    max_row = row;
                }
            }

            if aug[[max_row, col]].abs() < 1e-10 {
                return None;
            }

            if max_row != col {
                for j in 0..2 * n {
                    let tmp = aug[[col, j]];
                    aug[[col, j]] = aug[[max_row, j]];
                    aug[[max_row, j]] = tmp;
                }
            }

            let pivot = aug[[col, col]];
            for j in 0..2 * n {
                aug[[col, j]] /= pivot;
            }

            for row in 0..n {
                if row != col {
                    let factor = aug[[row, col]];
                    for j in 0..2 * n {
                        aug[[row, j]] -= factor * aug[[col, j]];
                    }
                }
            }
        }

        let mut inv = Array2::<f64>::zeros((n, n));
        for i in 0..n {
            for j in 0..n {
                inv[[i, j]] = aug[[i, n + j]];
            }
        }

        Some(inv)
    }

    pub fn fit_variogram(
        &mut self,
        known_points: &[(f64, f64, f64)],
    ) -> Result<(), SignalProcessingError> {
        let n = known_points.len();
        let mut distances = Vec::new();
        let mut variogram_values = Vec::new();

        for i in 0..n {
            for j in (i + 1)..n {
                let dist = Self::euclidean_distance(
                    (known_points[i].0, known_points[i].1),
                    (known_points[j].0, known_points[j].1),
                );
                let var = (known_points[i].2 - known_points[j].2).powi(2) / 2.0;
                distances.push(dist);
                variogram_values.push(var);
            }
        }

        if distances.len() < 10 {
            return Err(SignalProcessingError::NormalizationError(
                "样本点太少，无法拟合变异函数".to_string()
            ));
        }

        let max_dist = distances.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        self.range = max_dist * 0.6;
        self.sill = variogram_values.iter().cloned().fold(0.0, f64::max);
        self.nugget = variogram_values.iter().take(10).sum::<f64>() / 10.0 * 0.1;

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub enum VariogramModel {
    Spherical,
    Exponential,
    Gaussian,
    Linear,
}

#[derive(Debug, Clone)]
pub struct StrainReading {
    pub position_z: f64,
    pub position_y: f64,
    pub strain_value: f64,
    pub temperature: f64,
}

#[derive(Debug, Clone)]
pub struct StrainField {
    pub values: Vec<f64>,
    pub variances: Vec<f64>,
    pub resolution: usize,
    pub blade_length: f64,
    pub blade_chord: f64,
}

impl StrainField {
    pub fn get_value_at(&self, z: f64, y: f64) -> Option<f64> {
        let ni = ((z / self.blade_length + 0.5) * (self.resolution - 1) as f64) as usize;
        let nj = ((y / self.blade_chord + 0.5) * (self.resolution - 1) as f64) as usize;
        
        if ni < self.resolution && nj < self.resolution {
            Some(self.values[ni * self.resolution + nj])
        } else {
            None
        }
    }

    pub fn get_gradient(&self, z: f64, y: f64) -> Option<(f64, f64)> {
        let step = 2.0 / (self.resolution - 1) as f64;
        let v_center = self.get_value_at(z, y)?;
        let v_z = self.get_value_at(z + step * self.blade_length * 0.5, y)?;
        let v_y = self.get_value_at(z, y + step * self.blade_chord * 0.5)?;
        
        let dz = (v_z - v_center) / (step * self.blade_length * 0.5);
        let dy = (v_y - v_center) / (step * self.blade_chord * 0.5);
        
        Some((dz, dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn test_wavelet_denoise() {
        let processor = SignalProcessor::new();
        let signal: Vec<f64> = (0..32).map(|i| (i as f64 * 0.3).sin() + 0.1 * (i as f64 * 2.0).sin()).collect();
        
        let result = processor.wavelet_denoise(&signal, 2, ThresholdMethod::Universal);
        assert!(result.is_ok());
        let denoised = result.unwrap();
        assert_eq!(denoised.len(), signal.len());
    }

    #[test]
    fn test_normalize_ae_features() {
        let processor = SignalProcessor::new();
        
        let result = processor.normalize_ae_features(80.0, 1000.0, 200.0, 5000.0, 8.0, 12.0);
        assert!(result.is_ok());
        
        let (amp, dur, freq, energy) = result.unwrap();
        assert_relative_eq!(amp, 80.0, epsilon = 1e-6);
        assert_relative_eq!(dur, 1000.0, epsilon = 1e-6);
    }

    #[test]
    fn test_kriging_interpolation() {
        let interpolator = KrigingInterpolator::new();
        
        let known_points = vec![
            (-1.0, -0.5, 100.0),
            (-1.0, 0.5, 120.0),
            (0.0, 0.0, 150.0),
            (1.0, -0.5, 180.0),
            (1.0, 0.5, 200.0),
        ];
        
        let query_points = vec![(0.0, 0.0), (0.5, 0.25)];
        
        let result = interpolator.interpolate(&known_points, &query_points);
        assert!(result.is_ok());
        
        let interpolated = result.unwrap();
        assert_eq!(interpolated.len(), 2);
        assert!(interpolated[0].0 > 100.0 && interpolated[0].0 < 200.0);
    }

    #[test]
    fn test_order_tracking() {
        let tracker = OrderTracker::new(1000.0, 10.0, 0.1);
        
        let signal: Vec<f64> = (0..256).map(|i| {
            let t = i as f64 / 1000.0;
            (2.0 * PI * 12.5 * t).sin() + 0.5 * (2.0 * PI * 25.0 * t).sin()
        }).collect();
        
        let spectrum = tracker.compute_order_spectrum(&signal, 12.0, None);
        assert!(spectrum.is_ok());
    }
}
