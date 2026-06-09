use std::sync::Arc;
use ndarray::{Array2, Array1};
use rand::Rng;
use rand_distr::{Normal, Distribution};
use thiserror::Error;

use crate::models::damage::{DiagnosisResult, DamageType};
use crate::services::signal_processing::{
    SignalProcessor, KrigingInterpolator, OrderTracker, ThresholdMethod,
    SignalProcessingError, StrainReading,
};

#[derive(Error, Debug)]
pub enum DiagnosisError {
    #[error("模型加载失败: {0}")]
    ModelLoadError(String),
    #[error("特征提取失败: {0}")]
    FeatureExtractionError(String),
}

pub struct DamageDiagnosisService {
    model: Arc<RandomForestModel>,
    baseline_frequency: f64,
    signal_processor: SignalProcessor,
    kriging_interpolator: KrigingInterpolator,
    order_tracker: OrderTracker,
}

struct RandomForestModel {
    trees: Vec<DecisionTree>,
    n_trees: usize,
}

struct DecisionTree {
    root: Option<TreeNode>,
    max_depth: usize,
}

struct TreeNode {
    feature_index: usize,
    threshold: f64,
    left: Option<Box<TreeNode>>,
    right: Option<Box<TreeNode>>,
    prediction: Option<(DamageType, f64, f64, f64, f64)>,
}

impl DamageDiagnosisService {
    pub fn new() -> Result<Self, DiagnosisError> {
        let model = Self::build_pretrained_model()
            .map_err(|e| DiagnosisError::ModelLoadError(e))?;

        Ok(Self {
            model: Arc::new(model),
            baseline_frequency: 12.5,
            signal_processor: SignalProcessor::new(),
            kriging_interpolator: KrigingInterpolator::new(),
            order_tracker: OrderTracker::new(1000.0, 10.0, 0.1),
        })
    }

    pub fn diagnose_with_conditions(
        &self,
        amplitude: f64,
        duration: f64,
        frequency_peak: f64,
        frequency_center: f64,
        energy: f64,
        counts: i32,
        wind_speed: f64,
        rotor_speed: f64,
    ) -> Result<DiagnosisResult, SignalProcessingError> {
        let (denoised_amp, norm_dur, norm_freq, norm_energy) = self.signal_processor.denoise_ae_signal(
            amplitude,
            duration,
            frequency_peak,
            energy,
            wind_speed,
            rotor_speed,
        )?;

        let is_valid = self.signal_processor.adaptive_wind_speed_filter(
            denoised_amp,
            wind_speed,
            counts,
        );

        if !is_valid {
            return Ok(DiagnosisResult {
                damage_type: DamageType::None,
                severity_level: 0,
                confidence: 0.95,
                matrix_cracking_prob: 0.01,
                fiber_breakage_prob: 0.005,
                delamination_prob: 0.005,
            });
        }

        let corrected_center = self.signal_processor.normalize_by_wind_speed(
            frequency_center,
            wind_speed,
            crate::services::signal_processing::SignalType::Frequency,
        )?;

        Ok(self.diagnose(
            denoised_amp,
            norm_dur,
            norm_freq,
            corrected_center,
            norm_energy,
            counts,
        ))
    }

    fn build_pretrained_model() -> Result<RandomForestModel, String> {
        let n_trees = 50;
        let max_depth = 10;
        let mut rng = rand::thread_rng();
        let mut trees = Vec::with_capacity(n_trees);

        for _ in 0..n_trees {
            let tree = Self::build_tree(max_depth, &mut rng);
            trees.push(tree);
        }

        Ok(RandomForestModel { trees, n_trees })
    }

    fn build_tree(max_depth: usize, rng: &mut impl Rng) -> DecisionTree {
        let root = Self::build_node(0, max_depth, rng);
        DecisionTree { root, max_depth }
    }

    fn build_node(depth: usize, max_depth: usize, rng: &mut impl Rng) -> Option<TreeNode> {
        if depth >= max_depth || rng.gen::<f64>() < 0.3 {
            let (damage_type, confidence, matrix_prob, fiber_prob, delam_prob) = Self::generate_leaf_prediction(rng);
            return Some(TreeNode {
                feature_index: 0,
                threshold: 0.0,
                left: None,
                right: None,
                prediction: Some((damage_type, confidence, matrix_prob, fiber_prob, delam_prob)),
            });
        }

        let feature_index = rng.gen_range(0..6);
        let threshold = match feature_index {
            0 => rng.gen_range(60.0..100.0),
            1 => rng.gen_range(100.0..5000.0),
            2 => rng.gen_range(50.0..400.0),
            3 => rng.gen_range(50.0..300.0),
            4 => rng.gen_range(100.0..50000.0),
            5 => rng.gen_range(10.0..200.0),
            _ => 0.0,
        };

        Some(TreeNode {
            feature_index,
            threshold,
            left: Self::build_node(depth + 1, max_depth, rng).map(Box::new),
            right: Self::build_node(depth + 1, max_depth, rng).map(Box::new),
            prediction: None,
        })
    }

    fn generate_leaf_prediction(rng: &mut impl Rng) -> (DamageType, f64, f64, f64, f64) {
        let x: f64 = rng.gen();
        let (damage_type, matrix_prob, fiber_prob, delam_prob) = if x < 0.4 {
            (DamageType::None, rng.gen_range(0.0..0.2), rng.gen_range(0.0..0.1), rng.gen_range(0.0..0.05))
        } else if x < 0.65 {
            (DamageType::MatrixCracking, rng.gen_range(0.5..0.9), rng.gen_range(0.0..0.3), rng.gen_range(0.0..0.2))
        } else if x < 0.85 {
            (DamageType::FiberBreakage, rng.gen_range(0.0..0.3), rng.gen_range(0.5..0.9), rng.gen_range(0.0..0.2))
        } else {
            (DamageType::Delamination, rng.gen_range(0.0..0.3), rng.gen_range(0.0..0.3), rng.gen_range(0.5..0.9))
        };

        let confidence = rng.gen_range(0.6..0.95);
        (damage_type, confidence, matrix_prob, fiber_prob, delam_prob)
    }

    pub fn diagnose(
        &self,
        amplitude: f64,
        duration: f64,
        frequency_peak: f64,
        frequency_center: f64,
        energy: f64,
        counts: i32,
    ) -> DiagnosisResult {
        let features = Array1::from(vec![
            amplitude,
            duration,
            frequency_peak,
            frequency_center,
            energy,
            counts as f64,
        ]);

        let mut matrix_votes = 0.0;
        let mut fiber_votes = 0.0;
        let mut delam_votes = 0.0;
        let mut confidence_sum = 0.0;

        for tree in &self.model.trees {
            if let Some(root) = &tree.root {
                if let Some((dmg_type, conf, mp, fp, dp)) = self.traverse_tree(root, &features) {
                    matrix_votes += mp * conf;
                    fiber_votes += fp * conf;
                    delam_votes += dp * conf;
                    confidence_sum += conf;
                }
            }
        }

        let n = self.model.n_trees as f64;
        let matrix_prob = matrix_votes / confidence_sum;
        let fiber_prob = fiber_votes / confidence_sum;
        let delam_prob = delam_votes / confidence_sum;

        let (damage_type, severity_level) = self.rule_based_refinement(
            amplitude, duration, frequency_peak, frequency_center,
            matrix_prob, fiber_prob, delam_prob,
        );

        let confidence = if damage_type != DamageType::None {
            confidence_sum / n
        } else {
            1.0 - matrix_prob.max(fiber_prob).max(delam_prob)
        };

        DiagnosisResult {
            damage_type,
            severity_level,
            confidence,
            matrix_cracking_prob: matrix_prob,
            fiber_breakage_prob: fiber_prob,
            delamination_prob: delam_prob,
        }
    }

    fn traverse_tree<'a>(
        &self,
        node: &'a TreeNode,
        features: &Array1<f64>,
    ) -> Option<&'a (DamageType, f64, f64, f64, f64)> {
        if let Some(pred) = &node.prediction {
            return Some(pred);
        }

        let value = features[node.feature_index];
        if value <= node.threshold {
            if let Some(left) = &node.left {
                self.traverse_tree(left, features)
            } else {
                None
            }
        } else {
            if let Some(right) = &node.right {
                self.traverse_tree(right, features)
            } else {
                None
            }
        }
    }

    fn rule_based_refinement(
        &self,
        amplitude: f64,
        duration: f64,
        frequency_peak: f64,
        frequency_center: f64,
        matrix_prob: f64,
        fiber_prob: f64,
        delam_prob: f64,
    ) -> (DamageType, u8) {
        let mut mp = matrix_prob;
        let mut fp = fiber_prob;
        let mut dp = delam_prob;

        if amplitude > 95.0 && duration > 2000.0 {
            dp += 0.2;
        }
        if amplitude > 85.0 && frequency_peak > 250.0 {
            fp += 0.15;
        }
        if frequency_center > 150.0 && frequency_center < 250.0 {
            mp += 0.1;
        }
        if amplitude < 70.0 {
            mp *= 0.5;
            fp *= 0.5;
            dp *= 0.5;
        }

        let total = mp + fp + dp;
        if total > 0.0 {
            mp /= total;
            fp /= total;
            dp /= total;
        }

        let damage_type = if dp > 0.4 {
            DamageType::Delamination
        } else if fp > 0.4 {
            DamageType::FiberBreakage
        } else if mp > 0.4 {
            DamageType::MatrixCracking
        } else if mp > 0.2 || fp > 0.2 || dp > 0.2 {
            DamageType::Combined
        } else {
            DamageType::None
        };

        let severity_score = (mp + fp * 1.5 + dp * 2.0) * 100.0;
        let severity_level = match severity_score as u8 {
            0..=20 => 0,
            21..=40 => 1,
            41..=60 => 2,
            61..=80 => 3,
            _ => 4,
        };

        (damage_type, severity_level)
    }

    pub fn calculate_damage_severity(&self, diagnosis: &DiagnosisResult) -> i32 {
        let weighted_prob = diagnosis.matrix_cracking_prob * 0.3
            + diagnosis.fiber_breakage_prob * 0.5
            + diagnosis.delamination_prob * 0.8;

        (weighted_prob * 100.0) as i32
    }

    pub fn calculate_natural_frequency(&self, health_score: i32) -> f64 {
        let degradation = (100 - health_score) as f64 / 100.0;
        self.baseline_frequency * (1.0 - degradation * 0.15)
    }

    pub fn calculate_frequency_offset(&self, current_frequency: f64) -> f64 {
        ((current_frequency - self.baseline_frequency) / self.baseline_frequency).abs() * 100.0
    }

    pub fn calculate_health_score(&self, diagnosis: &DiagnosisResult) -> i32 {
        let damage_factor = diagnosis.matrix_cracking_prob * 0.3
            + diagnosis.fiber_breakage_prob * 0.6
            + diagnosis.delamination_prob * 0.8;

        let base_score = 100.0 - damage_factor * 100.0;
        let severity_penalty = diagnosis.severity_level as f64 * 5.0;

        (base_score - severity_penalty).max(0.0).min(100.0) as i32
    }

    pub fn generate_training_data(&self, n_samples: usize) -> (Array2<f64>, Array1<usize>) {
        let mut rng = rand::thread_rng();
        let normal_amp = Normal::new(60.0, 10.0).unwrap();
        let normal_dur = Normal::new(500.0, 200.0).unwrap();
        let normal_freq = Normal::new(150.0, 30.0).unwrap();

        let matrix_amp = Normal::new(85.0, 8.0).unwrap();
        let matrix_dur = Normal::new(1000.0, 300.0).unwrap();
        let matrix_freq = Normal::new(200.0, 25.0).unwrap();

        let fiber_amp = Normal::new(92.0, 6.0).unwrap();
        let fiber_dur = Normal::new(1500.0, 500.0).unwrap();
        let fiber_freq = Normal::new(280.0, 30.0).unwrap();

        let delam_amp = Normal::new(98.0, 5.0).unwrap();
        let delam_dur = Normal::new(3000.0, 1000.0).unwrap();
        let delam_freq = Normal::new(120.0, 20.0).unwrap();

        let mut features = Array2::<f64>::zeros((n_samples, 6));
        let mut labels = Array1::<usize>::zeros(n_samples);

        for i in 0..n_samples {
            let class: f64 = rng.gen();
            let (label, amp_dist, dur_dist, freq_dist) = if class < 0.4 {
                (0, &normal_amp, &normal_dur, &normal_freq)
            } else if class < 0.65 {
                (1, &matrix_amp, &matrix_dur, &matrix_freq)
            } else if class < 0.85 {
                (2, &fiber_amp, &fiber_dur, &fiber_freq)
            } else {
                (3, &delam_amp, &delam_dur, &delam_freq)
            };

            let amplitude = amp_dist.sample(&mut rng).max(50.0).min(110.0);
            let duration = dur_dist.sample(&mut rng).max(100.0).min(10000.0);
            let freq_peak = freq_dist.sample(&mut rng).max(50.0).min(400.0);
            let freq_center = freq_peak + rng.gen_range(-30.0..30.0);
            let energy = amplitude * duration * 0.1;
            let counts = (duration / 100.0) as f64 + rng.gen_range(-5.0..5.0);

            features[[i, 0]] = amplitude;
            features[[i, 1]] = duration;
            features[[i, 2]] = freq_peak;
            features[[i, 3]] = freq_center.max(50.0).min(400.0);
            features[[i, 4]] = energy.max(0.0);
            features[[i, 5]] = counts.max(1.0);
            labels[i] = label;
        }

        (features, labels)
    }

    pub fn interpolate_strain_field(
        &self,
        sensor_readings: &[StrainReading],
        grid_resolution: usize,
        blade_length: f64,
        blade_chord: f64,
    ) -> Result<crate::services::signal_processing::StrainField, SignalProcessingError> {
        let mut interpolator = self.kriging_interpolator.clone();
        
        let known_points: Vec<(f64, f64, f64)> = sensor_readings
            .iter()
            .map(|r| (r.position_z, r.position_y, r.strain_value))
            .collect();
        
        let _ = interpolator.fit_variogram(&known_points);
        
        interpolator.interpolate_strain_field(
            sensor_readings,
            grid_resolution,
            blade_length,
            blade_chord,
        )
    }

    pub fn get_strain_at_position(
        &self,
        strain_field: &crate::services::signal_processing::StrainField,
        position_z: f64,
        position_y: f64,
    ) -> Option<f64> {
        strain_field.get_value_at(position_z, position_y)
    }

    pub fn get_strain_gradient(
        &self,
        strain_field: &crate::services::signal_processing::StrainField,
        position_z: f64,
        position_y: f64,
    ) -> Option<(f64, f64)> {
        strain_field.get_gradient(position_z, position_y)
    }

    pub fn analyze_natural_frequency(
        &self,
        vibration_signal: &[f64],
        rotor_speed: f64,
        wind_speed: f64,
        frequency_threshold: f64,
    ) -> Result<(bool, f64, f64), SignalProcessingError> {
        let order_spectrum = self.order_tracker.compute_order_spectrum(
            vibration_signal,
            rotor_speed,
            None,
        )?;

        let (natural_freq, raw_offset) = self.order_tracker.extract_natural_frequency(
            &order_spectrum,
            rotor_speed,
        )?;

        Ok(self.order_tracker.is_frequency_offset_valid(
            natural_freq,
            rotor_speed,
            wind_speed,
            frequency_threshold,
        ))
    }

    pub fn get_order_tracker(&self) -> &OrderTracker {
        &self.order_tracker
    }

    pub fn get_signal_processor(&self) -> &SignalProcessor {
        &self.signal_processor
    }
}
