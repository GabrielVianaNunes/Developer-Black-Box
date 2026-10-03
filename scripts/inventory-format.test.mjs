// Formatação dos valores numéricos do inventário (sem texto vindo do backend).
import assert from "node:assert/strict";
import { test } from "node:test";

import { inventoryValueText, versionText } from "../src/i18n/inventory.ts";

const t = (key, params) => (params ? `${key}:${JSON.stringify(params)}` : key);
const pack = (a, b = 0, c = 0, d = 0) => ((a * 8192 + b) * 8192 + c) * 8192 + d;

test("packed versions are shown dotted and without trailing zeros", () => {
  assert.equal(versionText(pack(1, 22, 333), t), "1.22.333");
  assert.equal(versionText(pack(7), t), "7");
  assert.equal(versionText(pack(8191, 1, 2, 3), t), "8191.1.2.3");
});

test("a hashed version is shown as a non-numeric change, never as numbers", () => {
  assert.equal(versionText(2 ** 52 + 12345, t), "inventory.value.hashed");
});

test("each item is formatted from its number", () => {
  assert.equal(inventoryValueText("BiosDate", 20240307, t), "2024-03-07");
  assert.equal(inventoryValueText("FirmwareType", 2, t), "inventory.value.uefi");
  assert.equal(inventoryValueText("FirmwareType", 1, t), "inventory.value.legacy");
  assert.equal(inventoryValueText("SecureBoot", 1, t), "inventory.value.on");
  assert.equal(inventoryValueText("SecureBoot", 0, t), "inventory.value.off");
  assert.equal(inventoryValueText("OsBuild", 26100 * 2 ** 20 + 1742, t), "26100.1742");
  assert.equal(inventoryValueText("DeviceProblemCount", 3, t), "3");
});

test("the problem-code mask is listed code by code", () => {
  assert.equal(inventoryValueText("DeviceProblemCodes", 2 ** 10 + 2 ** 43, t), "10, 43");
  assert.equal(inventoryValueText("DeviceProblemCodes", 1 + 2 ** 52, t), "53+, 52");
  assert.equal(inventoryValueText("DeviceProblemCodes", 0, t), "inventory.value.none");
});

test("an unreadable value is shown as unavailable", () => {
  assert.equal(inventoryValueText("OsBuild", null, t), "inventory.value.unavailable");
});
