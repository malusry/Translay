const assert = require('node:assert/strict');

// Chromium reports the font that actually supplied glyphs, including fallback.
// A computed CSS family alone cannot prove that a local font loaded.
async function renderedFonts(page, selector) {
  const session = await page.context().newCDPSession(page);
  try {
    await session.send('DOM.enable');
    await session.send('CSS.enable');
    const { root } = await session.send('DOM.getDocument');
    const { nodeId } = await session.send('DOM.querySelector', { nodeId: root.nodeId, selector });
    assert(nodeId, `Missing font sample: ${selector}`);
    const { fonts } = await session.send('CSS.getPlatformFontsForNode', { nodeId });
    return fonts;
  } finally { await session.detach(); }
}

async function assertFont(page, selector, family) {
  await page.evaluate(() => document.fonts.ready);
  const fonts = await renderedFonts(page, selector);
  assert(fonts.some(font => font.isCustomFont && font.familyName === family && font.glyphCount > 0), `${selector}: expected ${family}, got ${JSON.stringify(fonts)}`);
  return fonts;
}

module.exports = { assertFont, renderedFonts };
