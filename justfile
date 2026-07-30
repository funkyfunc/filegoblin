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
