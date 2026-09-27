# TextHalo Modal video generation

This directory contains a separate, on-demand Wan 2.2 video generator. It does
not modify the existing Flux image service or the H3 cache. The model is
downloaded once into its own Modal Volume by a CPU-only function; generation
uses one L40S GPU container and scales back to zero after the job.

Wan 2.2 TI2V-5B is licensed under Apache 2.0. This starter implementation
generates silent text-to-video MP4 clips. Add voice or music in the local
editor after generation.

## Requirements

- Modal CLI installed and authenticated with the intended Modal account.
- Modal CLI available on `PATH`.
- A payment method on the Modal account. Starter credits may offset usage, but
  they are not a hard spend limit.

## Deploy

From the repository root:

```sh
modal deploy modal-video/modal_app.py
```

The deployment creates two new volumes: `texthalo-wan-model-cache` and
`texthalo-wan-video-output`. It does not stop or replace other Modal apps.

## Generate

```sh
modal run modal-video/modal_app.py \
  --prompt "A cinematic product reveal of a glowing glass soundwave" \
  --output modal-video/output/texthalo-reveal.mp4
```

The first invocation downloads the approximately 34 GB model to the cache
volume without allocating a GPU. Later invocations reuse that cache. The
default is 480x832, 33 frames at 16 fps, and 20 inference steps (about two
seconds). Use `--frames 81` for about five seconds, or `--height 720 --width
1280` for 720p. Higher resolution, frame count, and step count increase GPU
time. Only one generation container can run at a time.

To reuse a model cache that has already been populated:

```sh
modal run modal-video/modal_app.py --skip-model-download --prompt "..."
```

All generated MP4 files are downloaded from the output volume to the local
destination. The remote output volume remains available for later downloads.

## Model details

- Model: [Wan-AI/Wan2.2-TI2V-5B-Diffusers](https://huggingface.co/Wan-AI/Wan2.2-TI2V-5B-Diffusers)
- Pinned revision: `b8fff7315c768468a5333511427288870b2e9635`
- License: Apache 2.0
- GPU: Modal L40S, maximum one container
- Inference endpoint: none; calls are made through authenticated Modal functions
