#!/usr/bin/env node
import { readFileSync } from 'node:fs';

const [logPath = 'bld/baremetal-logs/latest.log', vitePath = '/home/t4ce/REPOS/Parse5/pixi-render-tree.json'] =
  process.argv.slice(2);

function stableHashText(text) {
  let h = (2166136261 ^ text.length) >>> 0;
  for (let i = 0; i < text.length; i += 1) {
    h ^= text.charCodeAt(i) & 0xffff;
    h = Math.imul(h, 16777619) >>> 0;
  }
  return `0x${h.toString(16).padStart(8, '0')}`;
}

function normalizeJsonText(text) {
  return JSON.stringify(JSON.parse(text));
}

function readLastTrueOsTreeDump(path) {
  const lines = readFileSync(path, 'utf8').split(/\r?\n/);
  let current = null;
  let last = null;

  for (const line of lines) {
    const begin = line.match(/\[truesurfer widget-tree-dump begin\].*chunks=(\d+)/);
    if (begin) {
      current = { total: Number(begin[1]) || 0, chunks: new Map() };
      continue;
    }

    const chunk = line.match(/\[truesurfer widget-tree-dump chunk\].*index=(\d+)\/(\d+) json=(.*)$/);
    if (chunk && current) {
      current.total = Number(chunk[2]) || current.total;
      current.chunks.set(Number(chunk[1]), chunk[3] ?? '');
      continue;
    }

    if (line.includes('[truesurfer widget-tree-dump end]') && current) {
      last = current;
      current = null;
    }
  }

  if (!last) throw new Error(`no truesurfer widget-tree-dump found in ${path}`);
  const parts = [];
  for (let i = 1; i <= last.total; i += 1) {
    if (!last.chunks.has(i)) throw new Error(`missing TrueOS dump chunk ${i}/${last.total}`);
    parts.push(last.chunks.get(i));
  }
  return parts.join('');
}

function firstJsonDiff(a, b, path = '$') {
  if (Object.is(a, b)) return null;
  if (typeof a !== typeof b) return `${path}: type ${typeof a} != ${typeof b}`;
  if (a === null || b === null || typeof a !== 'object') {
    return `${path}: ${JSON.stringify(a)?.slice(0, 120)} != ${JSON.stringify(b)?.slice(0, 120)}`;
  }
  if (Array.isArray(a) !== Array.isArray(b)) return `${path}: array/object mismatch`;
  if (Array.isArray(a)) {
    if (a.length !== b.length) return `${path}.length: ${a.length} != ${b.length}`;
    for (let i = 0; i < a.length; i += 1) {
      const diff = firstJsonDiff(a[i], b[i], `${path}[${i}]`);
      if (diff) return diff;
    }
    return null;
  }
  const keys = Array.from(new Set([...Object.keys(a), ...Object.keys(b)])).sort();
  for (const key of keys) {
    if (!Object.prototype.hasOwnProperty.call(a, key)) return `${path}.${key}: missing in TrueOS`;
    if (!Object.prototype.hasOwnProperty.call(b, key)) return `${path}.${key}: missing in Vite`;
    const diff = firstJsonDiff(a[key], b[key], `${path}.${key}`);
    if (diff) return diff;
  }
  return null;
}

const trueOsJson = normalizeJsonText(readLastTrueOsTreeDump(logPath));
const viteJson = normalizeJsonText(readFileSync(vitePath, 'utf8'));
const equal = trueOsJson === viteJson;

console.log(`trueos bytes=${trueOsJson.length} hash=${stableHashText(trueOsJson)}`);
console.log(`vite   bytes=${viteJson.length} hash=${stableHashText(viteJson)}`);
console.log(`equal=${equal ? 1 : 0}`);

if (!equal) {
  const a = JSON.parse(trueOsJson);
  const b = JSON.parse(viteJson);
  console.log(`first_diff=${firstJsonDiff(a, b) ?? 'byte difference only'}`);
  const max = Math.min(trueOsJson.length, viteJson.length);
  let at = 0;
  while (at < max && trueOsJson.charCodeAt(at) === viteJson.charCodeAt(at)) at += 1;
  console.log(`first_byte=${at}`);
  console.log(`trueos_at=${JSON.stringify(trueOsJson.slice(Math.max(0, at - 80), at + 160))}`);
  console.log(`vite_at=${JSON.stringify(viteJson.slice(Math.max(0, at - 80), at + 160))}`);
}
