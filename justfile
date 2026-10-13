# filegoblin manual build assets

# Fetch the ocrs model weights used by the pure-Rust OCR fallback on
# Linux/Windows (`--features ocr`). macOS uses Apple Vision natively and
# needs no weights. The weights are downloaded, never committed.
get-brains:
	@echo "Fetching Goblin OCR brains from the ethereal web..."
	@mkdir -p assets
	curl -L https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten -o assets/text-detection.rten
	curl -L https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten -o assets/text-recognition.rten
	@echo "Brains deposited in the Horde (/assets)!"

install:
	@echo "Installing latest filegoblin locally..."
	cargo install --path crates/cli --locked --force

# Fetch the whisper model for --features transcribe (also auto-downloads on first use)
get-whisper:
	@mkdir -p ~/.filegoblin/models
	curl -L https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin -o ~/.filegoblin/models/ggml-large-v3-turbo-q5_0.bin

# Install the PDF vision models (layout, formulas, tables, OCR) for builds
# with `--features vision`: the archive attached to the latest GitHub release,
# checked against its published SHA-256, unpacked into ~/.filegoblin/models.
# Air-gapped machines: copy the archive over and unpack it the same way, or
# point FILEGOBLIN_MODELS at any directory holding the files.
get-vision-models:
	@mkdir -p ~/.filegoblin
	cd ~/.filegoblin && curl -fL -O https://github.com/funkyfunc/filegoblin/releases/latest/download/filegoblin-vision-models.tar.gz -O https://github.com/funkyfunc/filegoblin/releases/latest/download/filegoblin-vision-models.tar.gz.sha256
	cd ~/.filegoblin && (sha256sum -c filegoblin-vision-models.tar.gz.sha256 || shasum -a 256 -c filegoblin-vision-models.tar.gz.sha256)
	cd ~/.filegoblin && tar -xzf filegoblin-vision-models.tar.gz && rm filegoblin-vision-models.tar.gz filegoblin-vision-models.tar.gz.sha256
	@echo "Vision models installed in ~/.filegoblin/models"

# Build the vision models from their upstream sources instead (needs Python
# with torch, transformers, onnx, onnxruntime, onnxscript, pyyaml).
build-vision-models:
	python3 scripts/vision_models.py ~/.filegoblin/models
