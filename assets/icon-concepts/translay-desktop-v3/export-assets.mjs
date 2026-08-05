import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const sharpPackage = process.argv[2];

if (!sharpPackage) {
  throw new Error("Pass the absolute path to an installed sharp package.");
}

const sharp = require(sharpPackage);
const root = path.dirname(fileURLToPath(import.meta.url));
const pngDir = path.join(root, "png");
const qaDir = path.join(root, "qa");

await fs.mkdir(pngDir, { recursive: true });
await fs.mkdir(qaDir, { recursive: true });

const optical = (size) => path.join(root, "optical", `translay-desktop-v3-${size}.svg`);
const master = path.join(root, "translay-desktop-v3-master.svg");

const jobs = [
  { size: 16, source: optical(16) },
  { size: 20, source: optical(20) },
  { size: 24, source: optical(24) },
  { size: 32, source: optical(32) },
  { size: 40, source: optical(48) },
  { size: 48, source: optical(48) },
  { size: 64, source: optical(64) },
  { size: 128, source: master },
  { size: 256, source: master },
  { size: 512, source: master },
];

for (const job of jobs) {
  const output = path.join(pngDir, `${job.size}x${job.size}.png`);
  await sharp(job.source, { density: 384 })
    .resize(job.size, job.size, { fit: "contain" })
    .png({ compressionLevel: 9, adaptiveFiltering: true })
    .toFile(output);
}

const validation = [];

for (const job of jobs) {
  const output = path.join(pngDir, `${job.size}x${job.size}.png`);
  const image = sharp(output).ensureAlpha();
  const metadata = await image.metadata();
  const { data, info } = await image.raw().toBuffer({ resolveWithObject: true });
  const cornerAlpha = data[3];
  let visiblePixels = 0;

  for (let index = 3; index < data.length; index += info.channels) {
    if (data[index] > 0) visiblePixels += 1;
  }

  if (metadata.width !== job.size || metadata.height !== job.size) {
    throw new Error(`Unexpected canvas for ${job.size}px export.`);
  }

  if (cornerAlpha !== 0 || visiblePixels === 0) {
    throw new Error(`Transparency validation failed for ${job.size}px export.`);
  }

  validation.push({
    size: job.size,
    width: metadata.width,
    height: metadata.height,
    channels: metadata.channels,
    cornerAlpha,
    visiblePixels,
  });
}

const icoSizes = [16, 20, 24, 32, 40, 48, 64, 128, 256];
const icoFrames = await Promise.all(
  icoSizes.map(async (size) => ({
    size,
    data: await fs.readFile(path.join(pngDir, `${size}x${size}.png`)),
  })),
);

const icoHeader = Buffer.alloc(6 + icoFrames.length * 16);
icoHeader.writeUInt16LE(0, 0);
icoHeader.writeUInt16LE(1, 2);
icoHeader.writeUInt16LE(icoFrames.length, 4);

let icoOffset = icoHeader.length;

icoFrames.forEach((frame, index) => {
  const entry = 6 + index * 16;
  icoHeader.writeUInt8(frame.size >= 256 ? 0 : frame.size, entry);
  icoHeader.writeUInt8(frame.size >= 256 ? 0 : frame.size, entry + 1);
  icoHeader.writeUInt8(0, entry + 2);
  icoHeader.writeUInt8(0, entry + 3);
  icoHeader.writeUInt16LE(1, entry + 4);
  icoHeader.writeUInt16LE(32, entry + 6);
  icoHeader.writeUInt32LE(frame.data.length, entry + 8);
  icoHeader.writeUInt32LE(icoOffset, entry + 12);
  icoOffset += frame.data.length;
});

await fs.writeFile(
  path.join(root, "translay-desktop-v3.ico"),
  Buffer.concat([icoHeader, ...icoFrames.map((frame) => frame.data)]),
);

const sheetWidth = 1200;
const sheetHeight = 720;
const sheetSvg = Buffer.from(`
  <svg xmlns="http://www.w3.org/2000/svg" width="${sheetWidth}" height="${sheetHeight}">
    <defs>
      <linearGradient id="mixed" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#DDD2C6"/>
        <stop offset=".48" stop-color="#A2B4BD"/>
        <stop offset="1" stop-color="#455673"/>
      </linearGradient>
      <pattern id="checker" width="24" height="24" patternUnits="userSpaceOnUse">
        <rect width="24" height="24" fill="#F5F6F8"/>
        <path d="M0 0H12V12H0ZM12 12H24V24H12Z" fill="#DCE1E6"/>
      </pattern>
    </defs>
    <rect width="1200" height="720" fill="#FFFFFF"/>
    <rect y="72" width="300" height="648" fill="#EEF1F5"/>
    <rect x="300" y="72" width="300" height="648" fill="#242933"/>
    <rect x="600" y="72" width="300" height="648" fill="url(#mixed)"/>
    <rect x="900" y="72" width="300" height="648" fill="url(#checker)"/>
    <g font-family="Segoe UI, Arial, sans-serif" font-size="18" text-anchor="middle">
      <text x="150" y="45" fill="#273142">Light</text>
      <text x="450" y="45" fill="#273142">Dark</text>
      <text x="750" y="45" fill="#273142">Mixed</text>
      <text x="1050" y="45" fill="#273142">Transparency</text>
    </g>
    <g font-family="Segoe UI, Arial, sans-serif" font-size="15" text-anchor="middle">
      <text x="150" y="230" fill="#273142">64 px</text><text x="450" y="230" fill="#E9EEF2">64 px</text><text x="750" y="230" fill="#FFFFFF">64 px</text><text x="1050" y="230" fill="#273142">64 px</text>
      <text x="150" y="375" fill="#273142">48 px</text><text x="450" y="375" fill="#E9EEF2">48 px</text><text x="750" y="375" fill="#FFFFFF">48 px</text><text x="1050" y="375" fill="#273142">48 px</text>
      <text x="150" y="510" fill="#273142">32 px</text><text x="450" y="510" fill="#E9EEF2">32 px</text><text x="750" y="510" fill="#FFFFFF">32 px</text><text x="1050" y="510" fill="#273142">32 px</text>
      <text x="150" y="635" fill="#273142">20 px</text><text x="450" y="635" fill="#E9EEF2">20 px</text><text x="750" y="635" fill="#FFFFFF">20 px</text><text x="1050" y="635" fill="#273142">20 px</text>
    </g>
  </svg>
`);

const columns = [150, 450, 750, 1050];
const contactRows = [
  { size: 64, centerY: 160 },
  { size: 48, centerY: 315 },
  { size: 32, centerY: 455 },
  { size: 20, centerY: 585 },
];
const composites = [];

for (const row of contactRows) {
  const input = await fs.readFile(path.join(pngDir, `${row.size}x${row.size}.png`));
  for (const centerX of columns) {
    composites.push({
      input,
      left: Math.round(centerX - row.size / 2),
      top: Math.round(row.centerY - row.size / 2),
    });
  }
}

await sharp(sheetSvg)
  .composite(composites)
  .png({ compressionLevel: 9, adaptiveFiltering: true })
  .toFile(path.join(qaDir, "translay-desktop-v3-contact-sheet.png"));

console.log(JSON.stringify({ exports: jobs.length, icoFrames: icoFrames.length, validation }, null, 2));
