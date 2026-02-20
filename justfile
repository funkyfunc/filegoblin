# filegoblin manual build assets

get-brains:
	@echo "Fetching Goblin OCR brains from the ethereal web..."
	@mkdir -p assets
	curl -L https://unpkg.com/tesseract.js-core@6.1.2/tesseract-core-simd.wasm -o assets/tesseract-core-simd.wasm
	@echo "Brains deposited in the Horde (/assets)!"
