'use strict';

// PDF vision models for filegoblin: PP-DocLayoutV3 (layout and reading
// order), pix2text MFR (formulas to LaTeX), Table Transformer (tables) and
// PP-OCRv6 (scan OCR). The weights come in the @filegoblin/model-* packages,
// one per model (registries limit file sizes, so the two largest come in
// halves, which filegoblin joins when it loads them). Licenses:
// models/LICENSES.md.

const fs = require('fs');
const path = require('path');

const PARTS = require('./parts.json');

function paths() {
  const dirs = [];
  const own = path.join(__dirname, 'models');
  if (fs.existsSync(own)) dirs.push(own);
  for (const name of PARTS) {
    try {
      const dir = path.join(path.dirname(require.resolve(`${name}/package.json`)), 'models');
      if (fs.existsSync(dir)) dirs.push(dir);
    } catch (_) {
      // A part that failed to install: filegoblin reports the model as missing.
    }
  }
  return dirs;
}

module.exports = { paths };
