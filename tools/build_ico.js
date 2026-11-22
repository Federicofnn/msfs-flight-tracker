// Packs the generated PNGs into a multi-size Windows .ico.
const fs = require('fs');
const path = require('path');
const dir = 'assets/icon_png';
const sizes = [16, 24, 32, 48, 64, 128, 256];
const images = sizes.map((size) => ({
  size,
  data: fs.readFileSync(path.join(dir, `icon_${size}.png`)),
}));
const header = Buffer.alloc(6);
header.writeUInt16LE(0, 0);
header.writeUInt16LE(1, 2);
header.writeUInt16LE(images.length, 4);
const entries = [];
let offset = 6 + images.length * 16;
for (const image of images) {
  const entry = Buffer.alloc(16);
  entry.writeUInt8(image.size >= 256 ? 0 : image.size, 0);
  entry.writeUInt8(image.size >= 256 ? 0 : image.size, 1);
  entry.writeUInt8(0, 2);
  entry.writeUInt8(0, 3);
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(image.data.length, 8);
  entry.writeUInt32LE(offset, 12);
  offset += image.data.length;
  entries.push(entry);
}
fs.writeFileSync('assets/icon.ico', Buffer.concat([header, ...entries, ...images.map((i) => i.data)]));
console.log('assets/icon.ico', fs.statSync('assets/icon.ico').size, 'bytes');
