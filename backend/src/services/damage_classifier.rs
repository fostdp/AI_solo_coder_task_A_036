use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use thiserror::Error;
use serde::Deserialize;
use ndarray::Array1;

use crate::models::damage::{DamageFeatures, DiagnosisResult, DamageType};
use crate::services::ethernet_driver::ProcessedAEData;
use crate::services::signal_processing::{SignalProcessor, SignalProcessingError};

#[derive(Error, Debug)]
pub enum DamageClassifierError {
    #[error("信号处理失败: {0}")]
    SignalProcessingError(#[from] SignalProcessingError),
    #[error("模型配置错误: {0}")]
    ConfigError(String),
    #[error("通道发送失败: {0}")]
    ChannelSendError(String),
    #[error("分类失败: {0}")]
    ClassificationError(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct RandomForestConfig {
    pub n_trees: usize,
    pub max_depth: usize,
    pub feature_ranges: FeatureRanges,
    pub class_priors: ClassPriors,
    pub rule_weights: RuleWeights,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FeatureRanges {
    pub amplitude: (f64, f64),
    pub duration: (f64, f64),
    pub frequency_peak: (f64, f64),
    pub frequency_center: (f64, f64),
    pub energy: (f64, f64),
    pub counts: (f64, f64),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClassPriors {
    pub none: f64,
    pub matrix_cracking: f64,
    pub fiber_breakage: f64,
    pub delamination: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleWeights {
    pub delamination_amp_threshold: f64,
    pub delamination_dur_threshold: f64,
    pub delamination_bonus: f64,
    pub fiber_amp_threshold: f64,
    pub fiber_freq_threshold: f64,
    pub fiber_bonus: f64,
    pub matrix_freq_low: f64,
    pub matrix_freq_high: f64,
    pub matrix_bonus: f64,
    pub low_amp_threshold: f64,
    pub low_amp_multiplier: f64,
    pub severity_matrix_weight: f64,
    pub severity_fiber_weight: f64,
    pub severity_delamination_weight: f64,
}

impl Default for RandomForestConfig {
    fn default() -> Self {
        Self {
            n_trees: 50,
            max_depth: 10,
            feature_ranges: FeatureRanges {
                amplitude: (60.0, 100.0),
                duration: (100.0, 5000.0),
                frequency_peak: (50.0, 400.0),
                frequency_center: (50.0, 300.0),
                energy: (100.0, 50000.0),
                counts: (10.0, 200.0),
            },
            class_priors: ClassPriors {
                none: 0.4,
                matrix_cracking: 0.25,
                fiber_breakage: 0.2,
                delamination: 0.15,
            },
            rule_weights: RuleWeights {
                delamination_amp_threshold: 95.0,
                delamination_dur_threshold: 2000.0,
                delamination_bonus: 0.2,
                fiber_amp_threshold: 85.0,
                fiber_freq_threshold: 250.0,
                fiber_bonus: 0.15,
                matrix_freq_low: 150.0,
                matrix_freq_high: 250.0,
                matrix_bonus: 0.1,
                low_amp_threshold: 70.0,
                low_amp_multiplier: 0.5,
                severity_matrix_weight: 0.3,
                severity_fiber_weight: 0.5,
                severity_delamination_weight: 0.8,
            },
        }
    }
}

impl RandomForestConfig {
    pub fn from_toml(path: &str) -> Result<Self, DamageClassifierError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| DamageClassifierError::ConfigError(format!("读取配置文件失败: {}", e)))?;
        
        toml::from_str(&content)
            .map_err(|e| DamageClassifierError::ConfigError(format!("解析TOML失败: {}", e)))
    }

    pub fn from_toml_str(content: &str) -> Result<Self, DamageClassifierError> {
        toml::from_str(content)
            .map_err(|e| DamageClassifierError::ConfigError(format!("解析TOML失败: {}", e)))
    }
}

struct DecisionTreeNode {
    feature_index: usize,
    threshold: f64,
    left: Option<Box<DecisionTreeNode>>,
    right: Option<Box<DecisionTreeNode>>,
    prediction: Option<(DamageType, f64, f64, f64, f64)>,
}

struct DecisionTree {
    root: Option<DecisionTreeNode>,
    max_depth: usize,
}

struct RandomForestModel {
    trees: Vec<DecisionTree>,
    config: Arc<RandomForestConfig>,
}

pub struct DamageClassifier {
    model: Arc<RandomForestModel>,
    signal_processor: Arc<SignalProcessor>,
    damage_sender: mpsc::Sender<DamageFeatures>,
    baseline_frequency: f64,
}

impl DamageClassifier {
    pub fn new(
        config: Arc<RandomForestConfig>,
        signal_processor: Arc<SignalProcessor>,
        damage_sender: mpsc::Sender<DamageFeatures>,
    ) -> Result<Self, DamageClassifierError> {
        let model = Self::build_model(config.clone())?;
        
        Ok(Self {
            model: Arc::new(model),
            signal_processor,
            damage_sender,
            baseline_frequency: 12.5,
        })
    }

    fn build_model(config: Arc<RandomForestConfig>) -> Result<RandomForestModel, DamageClassifierError> {
        let mut rng = rand::thread_rng();
        let mut trees = Vec::with_capacity(config.n_trees);

        for _ in 0..config.n_trees {
            let tree = Self::build_tree(config.max_depth, &config.feature_ranges, &mut rng);
            trees.push(tree);
        }

        Ok(RandomForestModel { trees, config })
    }

    fn build_tree(
        max_depth: usize,
        ranges: &FeatureRanges,
        rng: &mut impl rand::Rng,
    ) -> DecisionTree {
        let root = Self::build_node(0, max_depth, ranges, rng);
        DecisionTree { root, max_depth }
    }

    fn build_node(
        depth: usize,
        max_depth: usize,
        ranges: &FeatureRanges,
        rng: &mut impl rand::Rng,
    ) -> Option<DecisionTreeNode> {
        if depth >= max_depth || rng.gen::<f64>() < 0.3 {
            let (damage_type, confidence, mp, fp, dp) = Self::generate_leaf_prediction(rng);
            return Some(DecisionTreeNode {
                feature_index: 0,
                threshold: 0.0,
                left: None,
                right: None,
                prediction: Some((damage_type, confidence, mp, fp, dp)),
            });
        }

        let feature_index = rng.gen_range(0..6);
        let range_pair = match feature_index {
            0 => ranges.amplitude,
            1 => ranges.duration,
            2 => ranges.frequency_peak,
            3 => ranges.frequency_center,
            4 => ranges.energy,
            5 => ranges.counts,
            _ => (0.0, 1.0),
        };

        let threshold = rng.gen_range(range_pair.0..range_pair.1);

        Some(DecisionTreeNode {
            feature_index,
            threshold,
            left: Self::build_node(depth + 1, max_depth, ranges, rng).map(Box::new),
            right: Self::build_node(depth + 1, max_depth, ranges, rng).map(Box::new),
            prediction: None,
        })
    }

    fn generate_leaf_prediction(
        rng: &mut impl rand::Rng,
    ) -> (DamageType, f64, f64, f64, f64) {
        let x: f64 = rng.gen();
        let (damage_type, mp, fp, dp) = if x < 0.4 {
            (DamageType::None, rng.gen_range(0.0..0.2), rng.gen_range(0.0..0.1), rng.gen_range(0.0..0.05))
        } else if x < 0.65 {
            (DamageType::MatrixCracking, rng.gen_range(0.5..0.9), rng.gen_range(0.0..0.3), rng.gen_range(0.0..0.2))
        } else if x < 0.85 {
            (DamageType::FiberBreakage, rng.gen_range(0.0..0.3), rng.gen_range(0.5..0.9), rng.gen_range(0.0..0.2))
        } else {
            (DamageType::Delamination, rng.gen_range(0.0..0.3), rng.gen_range(0.0..0.3), rng.gen_range(0.5..0.9))
        };

        let confidence = rng.gen_range(0.6..0.95);
        (damage_type, confidence, mp, fp, dp)
    }

    pub fn classify(&self, data: &ProcessedAEData) -> DiagnosisResult {
        if !data.is_valid {
            return DiagnosisResult {
                damage_type: DamageType::None,
                severity_level: 0,
                confidence: 0.95,
                matrix_cracking_prob: 0.01,
                fiber_breakage_prob: 0.005,
                delamination_prob: 0.005,
            };
        }

        let features = Array1::from(vec![
            data.denoised_amplitude,
            data.normalized_duration,
            data.normalized_frequency,
            data.event.frequency_center,
            data.normalized_energy,
            data.event.counts as f64,
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

        let n = self.model.config.n_trees as f64;
        let mut matrix_prob = matrix_votes / confidence_sum;
        let mut fiber_prob = fiber_votes / confidence_sum;
        let mut delam_prob = delam_votes / confidence_sum;

        let (damage_type, severity_level) = self.rule_based_refinement(
            data.denoised_amplitude,
            data.normalized_duration,
            data.normalized_frequency,
            data.event.frequency_center,
            matrix_prob,
            fiber_prob,
            delam_prob,
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
        node: &'a DecisionTreeNode,
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
        let rules = &self.model.config.rule_weights;
        let mut mp = matrix_prob;
        let mut fp = fiber_prob;
        let mut dp = delam_prob;

        if amplitude > rules.delamination_amp_threshold && duration > rules.delamination_dur_threshold {
            dp += rules.delamination_bonus;
        }
        if amplitude > rules.fiber_amp_threshold && frequency_peak > rules.fiber_freq_threshold {
            fp += rules.fiber_bonus;
        }
        if frequency_center > rules.matrix_freq_low && frequency_center < rules.matrix_freq_high {
            mp += rules.matrix_bonus;
        }
        if amplitude < rules.low_amp_threshold {
            mp *= rules.low_amp_multiplier;
            fp *= rules.low_amp_multiplier;
            dp *= rules.low_amp_multiplier;
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

        let severity_score = (mp * rules.severity_matrix_weight
            + fp * rules.severity_fiber_weight
            + dp * rules.severity_delamination_weight) * 100.0;

        let severity_level = match severity_score as u8 {
            0..=20 => 0,
            21..=40 => 1,
            41..=60 => 2,
            61..=80 => 3,
            _ => 4,
        };

        (damage_type, severity_level)
    }

    pub fn create_damage_features(
        &self,
        data: &ProcessedAEData,
        diagnosis: &DiagnosisResult,
    ) -> DamageFeatures {
        let health_score = self.calculate_health_score(diagnosis);
        let natural_frequency = self.calculate_natural_frequency(health_score);

        DamageFeatures {
            turbine_id: data.event.turbine_id.clone(),
            blade_id: data.event.blade_id.clone(),
            section: data.event.section.clone(),
            matrix_cracking_prob: diagnosis.matrix_cracking_prob,
            fiber_breakage_prob: diagnosis.fiber_breakage_prob,
            delamination_prob: diagnosis.delamination_prob,
            damage_severity: diagnosis.severity_level as i32 * 25,
            natural_frequency,
            delamination_rate: diagnosis.delamination_prob * 10.0,
            health_score,
            wind_speed: data.wind_speed,
            rotor_speed: data.rotor_speed,
            timestamp: data.event.timestamp,
        }
    }

    pub fn calculate_health_score(&self, diagnosis: &DiagnosisResult) -> i32 {
        let damage_factor = diagnosis.matrix_cracking_prob * 0.3
            + diagnosis.fiber_breakage_prob * 0.6
            + diagnosis.delamination_prob * 0.8;

        let base_score = 100.0 - damage_factor * 100.0;
        let severity_penalty = diagnosis.severity_level as f64 * 5.0;

        (base_score - severity_penalty).max(0.0).min(100.0) as i32
    }

    pub fn calculate_natural_frequency(&self, health_score: i32) -> f64 {
        let degradation = (100 - health_score) as f64 / 100.0;
        self.baseline_frequency * (1.0 - degradation * 0.15)
    }

    pub async fn process_ae_data(
        &self,
        data: ProcessedAEData,
    ) -> Result<(), DamageClassifierError> {
        log::debug!(
            "分类器处理声发射数据: {}-{} {} 幅值={:.1}dB",
            data.event.turbine_id,
            data.event.blade_id,
            data.event.section,
            data.denoised_amplitude
        );

        let diagnosis = self.classify(&data);
        let features = self.create_damage_features(&data, &diagnosis);

        log::info!(
            "分类完成: {}-{} {} 类型={:?} 严重度={} 健康度={}",
            features.turbine_id,
            features.blade_id,
            features.section,
            diagnosis.damage_type,
            diagnosis.severity_level,
            features.health_score
        );

        self.damage_sender
            .send(features)
            .await
            .map_err(|e| DamageClassifierError::ChannelSendError(e.to_string()))?;

        Ok(())
    }

    pub async fn process_damage_features(
        &self,
        features: DamageFeatures,
    ) -> Result<(), DamageClassifierError> {
        log::info!(
            "分类器收到外部损伤特征: {}-{} {}",
            features.turbine_id,
            features.blade_id,
            features.section
        );

        self.damage_sender
            .send(features)
            .await
            .map_err(|e| DamageClassifierError::ChannelSendError(e.to_string()))?;

        Ok(())
    }
}

pub async fn start_damage_classifier(
    classifier: Arc<DamageClassifier>,
    mut ae_receiver: mpsc::Receiver<ProcessedAEData>,
    mut damage_receiver: mpsc::Receiver<DamageFeatures>,
) -> Result<(), DamageClassifierError> {
    log::info!("损伤分类器任务启动");

    loop {
        tokio::select! {
            Some(ae_data) = ae_receiver.recv() => {
                if let Err(e) = classifier.process_ae_data(ae_data).await {
                    log::error!("损伤分类器处理声发射数据失败: {}", e);
                }
            }
            Some(features) = damage_receiver.recv() => {
                if let Err(e) = classifier.process_damage_features(features).await {
                    log::error!("损伤分类器处理损伤特征失败: {}", e);
                }
            }
            else => {
                log::info!("损伤分类器任务正常关闭");
                break;
            }
        }
    }

    Ok(())
}
