// 社区详情兼容判定的纯逻辑回归（node --test；Node 24 原生剥离 TS 类型）。
import assert from "node:assert/strict";
import test from "node:test";

import {
  dependencyAppliesToInstance,
  fileAppliesToInstance,
  hasDeclaredIncompat,
} from "../src/store/community/resourceCompat.ts";
import { loaderAccepts } from "../src/store/community/resourceKind.ts";

const fabric263 = { mc_version: "26.3", loader: "fabric" };
const neoforge263 = { mc_version: "26.3", loader: "neoforge" };
const quilt263 = { mc_version: "26.3", loader: "quilt" };

test("Quilt 只兼容 Fabric，反向不成立", () => {
  assert.equal(loaderAccepts("quilt", "fabric"), true);
  assert.equal(loaderAccepts("quilt", "quilt"), true);
  assert.equal(loaderAccepts("fabric", "quilt"), false);
  assert.equal(loaderAccepts("forge", "fabric"), false);
});

test("26.2 文件对 26.3 实例不可安装，但依赖仍按声明展示", () => {
  const file = { loaders: ["fabric", "neoforge", "quilt"], game_versions: ["26.2"] };
  assert.equal(fileAppliesToInstance(file, fabric263), false);
  // 原始 bug：依赖面板跟着 selectedFileApplies 一起隐藏；预览语义必须保留 Fabric API。
  assert.equal(dependencyAppliesToInstance(["fabric"], file, fabric263), true);
});

test("文件适用时才按目标 loader 裁掉不相关前置", () => {
  const file = { loaders: ["fabric", "neoforge", "quilt"], game_versions: ["26.3"] };
  assert.equal(fileAppliesToInstance(file, fabric263), true);
  assert.equal(dependencyAppliesToInstance(["fabric"], file, fabric263), true);
  assert.equal(dependencyAppliesToInstance(["fabric"], file, neoforge263), false);
  // Quilt 可加载 Fabric 前置。
  assert.equal(dependencyAppliesToInstance(["fabric"], file, quilt263), true);
});

test("空元数据按未知放行，不误藏依赖", () => {
  const file = { loaders: [], game_versions: [] };
  assert.equal(fileAppliesToInstance(file, fabric263), true);
  assert.equal(dependencyAppliesToInstance([], file, fabric263), true);
});

test("没有 incompatible 声明时跳过冲突检查", () => {
  assert.equal(
    hasDeclaredIncompat([{ relation: "required" }, { relation: "optional" }]),
    false,
  );
  assert.equal(hasDeclaredIncompat([{ relation: "incompatible" }]), true);
});
