//! Supported Kokoro graphs from the same pinned ONNX export used by downloads.
//! Filenames, sizes and SHA-256 digests were checked against its upstream LFS metadata.

pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

pub const MODELS: &[Model] = &[
    Model {
        id: "fp32",
        label: "Full precision (FP32)",
        description: "Default quality and speed. Largest download.",
        file: "onnx/model.onnx",
        bytes: 325_532_232,
        sha256: "8fbea51ea711f2af382e88c833d9e288c6dc82ce5e98421ea61c058ce21a34cb",
    },
    Model {
        id: "fp16",
        label: "Smaller download (FP16)",
        description: "About half the download size. Does not necessarily use less RAM.",
        file: "onnx/model_fp16.onnx",
        bytes: 163_234_740,
        sha256: "ba4527a874b42b21e35f468c10d326fdff3c7fc8cac1f85e9eb6c0dfc35c334a",
    },
    Model {
        id: "q8f16",
        label: "Low memory (Q8F16)",
        description: "Smallest download and lower memory use. Speech generation can be slower.",
        file: "onnx/model_q8f16.onnx",
        bytes: 86_033_585,
        sha256: "04c658aec1b6008857c2ad10f8c589d4180d0ec427e7e6118ceb487e215c3cd0",
    },
    Model {
        id: "quantized",
        label: "Low memory (INT8)",
        description: "Another compact option. Speech generation can be slower.",
        file: "onnx/model_quantized.onnx",
        bytes: 92_361_116,
        sha256: "fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478",
    },
];

pub fn get(id: &str) -> Result<&'static Model, String> {
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("Unsupported Kokoro model: {id}. Choose a model in Voices."))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelOption {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub bytes: u64,
    pub installed: bool,
    pub removable: bool,
}

pub fn catalog() -> Vec<ModelOption> {
    MODELS
        .iter()
        .map(|model| ModelOption {
            id: model.id,
            label: model.label,
            description: model.description,
            bytes: model.bytes,
            installed: crate::engine_paths::kokoro_installed_variant(model.id),
            removable: crate::engine_paths::kokoro_dir().is_some_and(|dir|
                crate::model_storage::has_variant(&dir, crate::config::Engine::Kokoro, model.id)),
        })
        .collect()
}
