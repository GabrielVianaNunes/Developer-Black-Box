// Modelo da aba "Saúde do sistema": só organiza o que o backend devolveu; sem alarme falso para fonte indisponível.
import assert from "node:assert/strict";
import { test } from "node:test";

import { en } from "../src/i18n/en.ts";
import { ptBR } from "../src/i18n/pt-BR.ts";
import { NEVER_RECORDED, NOT_MONITORED, SOURCES, STATES, orderedSources, splitTimeline, stateTone } from "../src/features/health/model.ts";

const row = (seq, tsUtcMs, kind) => ({ seq, tsUtcMs, kind, pid: null, exeName: null, detail: { code: "unknown" } });

test("only attention is highlighted; unavailable, paused, waiting and off are neutral", () => {
  assert.equal(stateTone("attention"), "warn");
  assert.equal(stateTone("ok"), "good");
  for (const s of ["unavailable", "paused", "waiting", "off", "something-new"]) assert.equal(stateTone(s), "neutral", s);
});

test("sources come in a fixed order and unknown ones are ignored, never invented", () => {
  const got = orderedSources([
    { source: "telemetry", state: "off" },
    { source: "eventLog", state: "ok" },
    { source: "futureSource", state: "ok" },
    { source: "power", state: "banana" },
  ]);
  assert.deepEqual(got, [
    { source: "eventLog", state: "ok" },
    { source: "telemetry", state: "off" },
  ]);
  assert.deepEqual(orderedSources([]), []);
});

test("the timeline is newest first and the inventory changes have their own list", () => {
  const { timeline, inventory } = splitTimeline([
    row(1, 100, "HealthEvent"),
    row(2, 300, "PowerStatus"),
    row(3, 200, "InventoryChange"),
    row(4, 400, "InventoryChange"),
    row(5, 250, "ProcessStarted"),
  ]);
  assert.deepEqual(timeline.map((r) => r.seq), [2, 1], "events only, newest first; anything else is left out");
  assert.deepEqual(inventory.map((r) => r.seq), [4, 3]);
});

test("every source, state and 'not monitored' item has a text in both languages", () => {
  const keys = [
    ...SOURCES.flatMap((s) => [`health.source.${s}`, `health.sourceHelp.${s}`]),
    ...STATES.map((s) => `health.state.${s}`),
    ...NOT_MONITORED.map((k) => `health.not.${k}`),
    ...NEVER_RECORDED.map((k) => `health.never.${k}`),
  ];
  for (const k of keys) {
    assert.ok(en[k] && ptBR[k], k);
  }
});

test("the 'what is not monitored' list says why kernel-driver sensor libraries are out", () => {
  assert.match(en["health.not.kernelDrivers"], /kernel driver/i);
  assert.match(ptBR["health.not.kernelDrivers"], /driver de kernel/i);
  assert.ok(NOT_MONITORED.includes("smart") && NOT_MONITORED.includes("tpm") && NOT_MONITORED.includes("wheaOperational"));
});

test("the closed list of what is never recorded names messages, identifiers and names", () => {
  assert.deepEqual([...NEVER_RECORDED], ["messages", "identifiers", "names", "perProgram"]);
});
