import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

function range(bytes, offset, length) {
  if (!Number.isSafeInteger(offset) || offset < 0 || length < 0 || offset + length > bytes.length) {
    throw new Error("Data ikon/PE tidak lengkap.");
  }
  return bytes.subarray(offset, offset + length);
}

export function icoImages(bytes) {
  range(bytes, 0, 6);
  const count = bytes.readUInt16LE(4);
  if (bytes.readUInt32LE(0) !== 0x00010000 || !count) throw new Error("Header ICO tidak valid.");
  range(bytes, 6, count * 16);
  return Array.from({ length: count }, (_, i) => {
    const entry = 6 + i * 16;
    return range(bytes, bytes.readUInt32LE(entry + 12), bytes.readUInt32LE(entry + 8));
  });
}

export function executableIconImages(bytes) {
  range(bytes, 0, 64);
  if (bytes.toString("ascii", 0, 2) !== "MZ") throw new Error("Berkas bukan executable Windows.");
  const pe = bytes.readUInt32LE(60);
  range(bytes, pe, 24);
  if (bytes.readUInt32LE(pe) !== 0x00004550) throw new Error("Header PE tidak valid.");
  const optional = pe + 24;
  const optionalSize = bytes.readUInt16LE(pe + 20);
  range(bytes, optional, optionalSize);
  const magic = bytes.readUInt16LE(optional);
  if (magic !== 0x10b && magic !== 0x20b) throw new Error("Format PE tidak didukung.");
  const directory = optional + (magic === 0x20b ? 112 : 96);
  if (directory + 24 > optional + optionalSize) throw new Error("Direktori resource PE tidak tersedia.");
  const resourceRva = bytes.readUInt32LE(directory + 16);
  const resourceSize = bytes.readUInt32LE(directory + 20);
  const sectionCount = bytes.readUInt16LE(pe + 6);
  range(bytes, optional + optionalSize, sectionCount * 40);
  const sections = Array.from({ length: sectionCount }, (_, i) => optional + optionalSize + i * 40);
  const mapped = (rva, size) => {
    for (const section of sections) {
      const start = bytes.readUInt32LE(section + 12);
      const rawSize = bytes.readUInt32LE(section + 16);
      if (rva >= start && rva + size <= start + rawSize) {
        const offset = bytes.readUInt32LE(section + 20) + rva - start;
        range(bytes, offset, size);
        return offset;
      }
    }
    throw new Error("Resource ikon PE tidak dapat dipetakan.");
  };
  if (!resourceRva || !resourceSize) throw new Error("Executable tidak memiliki resource ikon.");
  const base = mapped(resourceRva, resourceSize);
  const resources = [];
  const entryAt = (relative, size) => {
    if (relative + size > resourceSize) throw new Error("Direktori resource PE tidak lengkap.");
    range(bytes, base + relative, size);
    return base + relative;
  };
  const walk = (relative, path = []) => {
    if (path.length >= 3) throw new Error("Struktur resource ikon PE terlalu dalam.");
    const directoryOffset = entryAt(relative, 16);
    const count = bytes.readUInt16LE(directoryOffset + 12) + bytes.readUInt16LE(directoryOffset + 14);
    entryAt(relative + 16, count * 8);
    for (let i = 0; i < count; i++) {
      const entry = directoryOffset + 16 + i * 8;
      const id = bytes.readUInt32LE(entry);
      if (!path.length && id !== 3 && id !== 14) continue; // RT_ICON / RT_GROUP_ICON
      const child = bytes.readUInt32LE(entry + 4);
      const next = [...path, id];
      if (child & 0x80000000) walk(child & 0x7fffffff, next);
      else {
        const data = entryAt(child, 16);
        const size = bytes.readUInt32LE(data + 4);
        resources.push({ path: next, bytes: range(bytes, mapped(bytes.readUInt32LE(data), size), size) });
      }
    }
  };
  walk(0);
  const group = resources.find((resource) => resource.path[0] === 14);
  if (!group) throw new Error("Executable tidak memiliki grup ikon utama.");
  range(group.bytes, 0, 6);
  if (group.bytes.readUInt32LE(0) !== 0x00010000) throw new Error("Grup ikon PE tidak valid.");
  const count = group.bytes.readUInt16LE(4);
  range(group.bytes, 6, count * 14);
  return Array.from({ length: count }, (_, i) => {
    const entry = 6 + i * 14;
    const id = group.bytes.readUInt16LE(entry + 12);
    const candidates = resources.filter((resource) => resource.path[0] === 3 && resource.path[1] === id);
    const image = candidates.find((resource) => resource.path[2] === group.path[2]) ?? candidates[0];
    if (!image || image.bytes.length !== group.bytes.readUInt32LE(entry + 8)) {
      throw new Error("Resource frame ikon PE hilang atau tidak lengkap.");
    }
    return image.bytes;
  });
}

export function verifyWindowsIcon(executable, expectedIco) {
  const expected = icoImages(expectedIco).map((bytes) => bytes.toString("base64")).sort();
  const actual = executableIconImages(executable).map((bytes) => bytes.toString("base64")).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error("Ikon executable tidak cocok dengan icons/icon.ico; ikon lama/default tidak boleh didistribusikan.");
  }
  return actual.length;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv.length < 3) throw new Error("Berikan path executable aplikasi dan/atau installer yang diperiksa.");
    const icon = readFileSync(new URL("../src-tauri/icons/icon.ico", import.meta.url));
    for (const path of process.argv.slice(2)) {
      const count = verifyWindowsIcon(readFileSync(path), icon);
      console.log(`Ikon WhisperTube terverifikasi (${count} frame): ${path}`);
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
