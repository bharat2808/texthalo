//! Language-model variants from the pinned Chatterbox Multilingual ONNX export.
//! The encoder, decoder and embedding graphs are shared and remain full precision.
use crate::kokoro_models::ModelOption;

pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub graph: &'static str,
    pub graph_bytes: u64,
    pub graph_sha256: &'static str,
    pub weights: &'static str,
    pub weights_bytes: u64,
    pub weights_sha256: &'static str,
}

pub const MODELS: &[Model] = &[
    Model {
        id: "q4f16",
        label: "Smallest (Q4F16)",
        description:
            "Default and smallest download. Chatterbox still needs large shared speech models.",
        graph: "onnx/language_model_q4f16.onnx",
        graph_bytes: 229_388,
        graph_sha256: "3b78e9235be5e2e2a811e482399155cb30415f6d87c98c21d12bf48843fc928f",
        weights: "onnx/language_model_q4f16.onnx_data",
        weights_bytes: 304_737_408,
        weights_sha256: "bdbc79504d20742b5d028074b4f1cdca8872e013fdfbbcea6b8b03154fe85a42",
    },
    Model {
        id: "q4",
        label: "Compact (Q4)",
        description: "4-bit language model with a full-precision cache. Larger than Q4F16.",
        graph: "onnx/language_model_q4.onnx",
        graph_bytes: 227_911,
        graph_sha256: "7f8cdca83b2493536cbf3acf421199808a3d68736f55f4eabd20ef8a99da4313",
        weights: "onnx/language_model_q4.onnx_data",
        weights_bytes: 353_621_248,
        weights_sha256: "e79ab8784122a501718868b9631ff46e151c552d9b24e50f25d721f375e3526c",
    },
    Model {
        id: "fp16",
        label: "Half precision (FP16)",
        description: "Larger download and higher memory requirements than the compact options.",
        graph: "onnx/language_model_fp16.onnx",
        graph_bytes: 172_657,
        graph_sha256: "0c36a5bbbc2a4ed8c345033896612cd320fd0971a0f5e6447ab4cdd2d7f22e36",
        weights: "onnx/language_model_fp16.onnx_data",
        weights_bytes: 1_040_316_416,
        weights_sha256: "16dca11ae994e78427fa3090cc6faf347a15988ca40809c1bd9f2721f3b759a0",
    },
    Model {
        id: "fp32",
        label: "Full precision (FP32)",
        description: "Largest download. Requires the most memory for language-model weights.",
        graph: "onnx/language_model.onnx",
        graph_bytes: 171_387,
        graph_sha256: "861a34585605e8ad671051788afc495dcbeaee833a41523a1b33aded9c3babc7",
        weights: "onnx/language_model.onnx_data",
        weights_bytes: 2_080_632_832,
        weights_sha256: "b3556d41085196c122b7197e4d44ec4475b6d7cfe0971a70faa95caa38ad787a",
    },
];

pub fn get(id: &str) -> Result<&'static Model, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("Unsupported Chatterbox model: {id}. Choose a model in Voices."))
}

pub fn catalog() -> Vec<ModelOption> {
    MODELS
        .iter()
        .map(|model| ModelOption {
            id: model.id,
            label: model.label,
            description: model.description,
            bytes: crate::download::chatterbox_plan_for(model.id)
                .expect("catalog model")
                .iter()
                .map(|entry| entry.bytes)
                .sum(),
            installed: crate::engine_paths::chatterbox_installed_variant(model.id),
        })
        .collect()
}
