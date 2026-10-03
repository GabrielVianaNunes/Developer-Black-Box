// Resumo de uma amostra de contadores: só os números que existem, nunca um "0" inventado.
import assert from "node:assert/strict";
import { test } from "node:test";

import { sampleText } from "../src/i18n/telemetry.ts";

const t = (key, params) => (params ? `${key}${JSON.stringify(params)}` : key);
const none = {
  thermalKelvin: null, passiveLimitPct: null, cpuLoadPct: null, cpuPerfPct: null, cpuFreqMhz: null, memCommitPct: null,
  memAvailableMb: null, pageFaultsPerSec: null, diskLatencyUs: null, diskBusyPct: null, netErrors: null, gpuPct: null,
};

test("a sample with no counters says so", () => {
  assert.equal(sampleText(none, t), "sample.none");
});

test("only the counters that exist are shown", () => {
  assert.equal(sampleText({ ...none, cpuLoadPct: 23 }, t), 'sample.cpu{"pct":23}');
  assert.equal(sampleText({ ...none, cpuLoadPct: 23, gpuPct: 8 }, t), 'sample.cpu{"pct":23} · sample.gpu{"pct":8}');
});

test("temperature is shown in Celsius, from kelvin", () => {
  assert.equal(sampleText({ ...none, thermalKelvin: 318 }, t), 'sample.temp{"c":45}');
});

test("disk latency is shown in milliseconds, from microseconds", () => {
  assert.equal(sampleText({ ...none, diskLatencyUs: 2100 }, t), 'sample.latency{"ms":"2.1"}');
  assert.equal(sampleText({ ...none, diskLatencyUs: 0 }, t), 'sample.latency{"ms":"0.0"}', "zero is a real reading, not missing");
});
