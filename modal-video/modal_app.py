"""On-demand Wan 2.2 video generation on Modal for TextHalo."""

import subprocess
import uuid
from pathlib import Path

import modal


APP_NAME = "texthalo-wan-video"
MODEL_ID = "Wan-AI/Wan2.2-TI2V-5B-Diffusers"
MODEL_REVISION = "b8fff7315c768468a5333511427288870b2e9635"
MODEL_PATH = "/model-cache/wan2.2-ti2v-5b"
OUTPUT_PATH = "/outputs"
MODEL_VOLUME_NAME = "texthalo-wan-model-cache"
OUTPUT_VOLUME_NAME = "texthalo-wan-video-output"

app = modal.App(APP_NAME)
model_volume = modal.Volume.from_name(MODEL_VOLUME_NAME, create_if_missing=True)
output_volume = modal.Volume.from_name(OUTPUT_VOLUME_NAME, create_if_missing=True)

cpu_image = (
    modal.Image.debian_slim(python_version="3.11")
    .pip_install("huggingface_hub[hf_xet]>=0.34,<1")
)

gpu_image = (
    modal.Image.debian_slim(python_version="3.11")
    .apt_install("ffmpeg")
    .pip_install(
        "torch==2.7.1",
        index_url="https://download.pytorch.org/whl/cu128",
    )
    .pip_install(
        "diffusers>=0.35.0,<1",
        "transformers>=4.51.0,<5",
        "accelerate>=1.6.0,<2",
        "huggingface_hub[hf_xet]>=0.34,<1",
        "safetensors>=0.5,<1",
        "imageio[ffmpeg]>=2.37,<3",
    )
)


@app.function(
    image=cpu_image,
    volumes={"/model-cache": model_volume},
    cpu=4,
    memory=16384,
    timeout=7200,
    max_containers=1,
)
def ensure_model() -> str:
    """Download the pinned model to persistent storage without allocating a GPU."""
    import os

    from huggingface_hub import snapshot_download

    os.makedirs(MODEL_PATH, exist_ok=True)
    snapshot_download(
        repo_id=MODEL_ID,
        revision=MODEL_REVISION,
        local_dir=MODEL_PATH,
    )
    model_volume.commit()
    return f"{MODEL_ID}@{MODEL_REVISION} is cached in {MODEL_VOLUME_NAME}."


@app.function(
    image=gpu_image,
    gpu="L40S",
    volumes={"/model-cache": model_volume, OUTPUT_PATH: output_volume},
    cpu=8,
    memory=65536,
    timeout=1800,
    max_containers=1,
    scaledown_window=2,
)
def generate_video(
    prompt: str,
    job_id: str,
    negative_prompt: str = "",
    height: int = 480,
    width: int = 832,
    num_frames: int = 33,
    steps: int = 20,
    seed: int = 42,
) -> str:
    """Generate one silent MP4 using a single L40S container."""
    import os

    import torch
    from diffusers import WanPipeline
    from diffusers.utils import export_to_video

    if not os.path.isfile(os.path.join(MODEL_PATH, "model_index.json")):
        raise RuntimeError("Model cache is empty. Run ensure_model before generation.")

    pipe = WanPipeline.from_pretrained(
        MODEL_PATH,
        torch_dtype=torch.bfloat16,
        local_files_only=True,
    ).to("cuda")
    generator = torch.Generator(device="cuda").manual_seed(seed)
    result = pipe(
        prompt=prompt,
        negative_prompt=negative_prompt or None,
        height=height,
        width=width,
        num_frames=num_frames,
        num_inference_steps=steps,
        guidance_scale=5.0,
        generator=generator,
    )

    destination = os.path.join(OUTPUT_PATH, f"{job_id}.mp4")
    export_to_video(result.frames[0], destination, fps=16)
    output_volume.commit()
    return os.path.basename(destination)


@app.local_entrypoint()
def main(
    prompt: str,
    negative_prompt: str = "",
    output: str = "",
    height: int = 480,
    width: int = 832,
    frames: int = 33,
    steps: int = 20,
    seed: int = 42,
    skip_model_download: bool = False,
):
    """Generate a clip and download it to this machine."""
    if not prompt.strip():
        raise ValueError("prompt cannot be empty")
    if len(prompt) > 2000 or len(negative_prompt) > 2000:
        raise ValueError("prompt text must be 2,000 characters or fewer")
    if (height, width) not in {(480, 832), (720, 1280)}:
        raise ValueError("supported resolutions: 480x832 or 720x1280")
    if frames not in {17, 33, 49, 65, 81}:
        raise ValueError("frames must be one of 17, 33, 49, 65, 81")
    if not 4 <= steps <= 50:
        raise ValueError("steps must be between 4 and 50")

    job_id = f"video-{uuid.uuid4().hex[:12]}"
    destination = Path(output) if output else Path(__file__).parent / "output" / f"{job_id}.mp4"
    destination.parent.mkdir(parents=True, exist_ok=True)

    if not skip_model_download:
        print("Checking the Modal model cache (CPU only)...", flush=True)
        print(modal.Function.from_name(APP_NAME, "ensure_model").remote(), flush=True)

    print("Generating video on one Modal L40S...", flush=True)
    remote_path = modal.Function.from_name(APP_NAME, "generate_video").remote(
        prompt=prompt.strip(),
        job_id=job_id,
        negative_prompt=negative_prompt.strip(),
        height=height,
        width=width,
        num_frames=frames,
        steps=steps,
        seed=seed,
    )
    subprocess.run(
        ["modal", "volume", "get", OUTPUT_VOLUME_NAME, remote_path, str(destination)],
        check=True,
    )
    print(f"Saved video to {destination.resolve()}")
