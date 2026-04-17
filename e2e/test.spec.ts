import { type ChildProcess, spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { type Key, launchTerminal, type Session } from "tuistory";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const __dirname = dirname(fileURLToPath(import.meta.url));
const FZFW_BIN = join(__dirname, "..", "target", "debug", "fzfw");
const PROJECT_ROOT = join(__dirname, "..");

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForSocket(
  path: string,
  timeoutMs: number,
): Promise<boolean> {
  const start = Date.now();
  while (!existsSync(path)) {
    if (Date.now() - start > timeoutMs) return false;
    await sleep(50);
  }
  return true;
}

interface TestContext {
  nvim: ChildProcess;
  nvimSock: string;
  tmpDir: string;
}

async function setupNvim(): Promise<TestContext> {
  const tmpDir = mkdtempSync(join(tmpdir(), "fzfw-e2e-"));
  const nvimSock = join(tmpDir, "nvim.sock");

  const nvimBin = process.env.NVIM_PATH || "nvim";
  const nvim = spawn(
    nvimBin,
    [
      "--headless",
      "--clean",
      "--cmd",
      "set shortmess+=F",
      "--listen",
      nvimSock,
    ],
    {
      stdio: "ignore",
    },
  );

  const ok = await waitForSocket(nvimSock, 5000);
  if (!ok) {
    nvim.kill();
    throw new Error("nvim socket not created");
  }

  return { nvim, nvimSock, tmpDir };
}

/** fzfwセッションを起動してメニューが表示されるまで待つ */
async function launchFzfw(ctx: TestContext): Promise<Session> {
  const session = await launchTerminal({
    command: FZFW_BIN,
    args: [],
    cols: 80,
    rows: 24,
    cwd: PROJECT_ROOT,
    env: {
      ...process.env,
      NVIM: ctx.nvimSock,
      FZFW_LOG_FILE: join(ctx.tmpDir, "fzfw-log"),
    },
  });
  await session.waitForText("menu>", { timeout: 10000 });
  return session;
}

/** プロンプト名を抽出（"menu>", "fd>", "livegrep>" 等） */
async function getPromptName(session: Session): Promise<string> {
  const text = await session.text({ trimEnd: true });
  const lines = text.split("\n");
  // fzfは1行目が空行、2行目がプロンプト
  const promptLine = lines[1] || "";
  const match = promptLine.match(/(\S+>)/);
  return match ? match[1] : promptLine;
}

/** ターミナル画面のうちアイテム一覧部分を取得 */
async function getItems(session: Session): Promise<string[]> {
  const text = await session.text({ trimEnd: true });
  const lines = text.split("\n");
  // 上4行（空行, prompt, count, header）を除いたアイテム行
  return lines.slice(4).filter((l) => l.trim().length > 0);
}

describe("fzfw e2e", () => {
  let ctx: TestContext;

  beforeAll(async () => {
    ctx = await setupNvim();
  });

  afterAll(() => {
    ctx.nvim.kill();
  });

  it("メニューモードが起動しモード一覧が表示される", async () => {
    const session = await launchFzfw(ctx);
    try {
      expect(await getPromptName(session)).toMatchInlineSnapshot(`"menu>"`);

      const items = await getItems(session);
      expect(items.length).toBeGreaterThan(10);
      expect(items.some((i) => i.includes("fd"))).toBe(true);
      expect(items.some((i) => i.includes("git-branch"))).toBe(true);
      expect(items.some((i) => i.includes("livegrep"))).toBe(true);
    } finally {
      session.close();
    }
  });

  it("pr-diffモードがメニューに表示される", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.type("pr-diff");
      await session.waitForText("pr-diff", { timeout: 5000 });

      const items = await getItems(session);
      expect(items.some((i) => i.includes("pr-diff"))).toBe(true);
    } finally {
      session.close();
    }
  });

  it("メニューで検索するとモード一覧がフィルタされる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.type("git");
      await session.waitForText("git-branch", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(`"menu>"`);

      const items = await getItems(session);
      expect(items.length).toBeLessThan(24);
      expect(items.length).toBeGreaterThan(0);
      expect(items.some((i) => i.includes("git-branch"))).toBe(true);
      expect(items.some((i) => i.includes("git-status"))).toBe(true);
      expect(items.some((i) => i.includes("▌ fd"))).toBe(false);
    } finally {
      session.close();
    }
  });

  it("Enterでメニューからfdモードに切り替わる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.type("fd");
      await session.waitForText("▌ fd", { timeout: 5000 });
      await session.press("enter");
      await session.waitForText("fd>", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(`"fd>"`);

      const items = await getItems(session);
      expect(items.length).toBeGreaterThan(0);
    } finally {
      session.close();
    }
  });

  it("Ctrl-Fでfdモードに直接切り替わる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.press(["ctrl", "f"]);
      await session.waitForText("fd>", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(`"fd>"`);
    } finally {
      session.close();
    }
  });

  it("PgDnでメニューに戻れる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.press(["ctrl", "f"]);
      await session.waitForText("fd>", { timeout: 5000 });

      await session.press("pagedown");
      await session.waitForText("menu>", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(`"menu>"`);
    } finally {
      session.close();
    }
  });

  it("Ctrl-Gでlivegrepモードに切り替わる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.press(["ctrl", "g"]);
      await session.waitForText("livegrep>", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(`"livegrep>"`);
    } finally {
      session.close();
    }
  });

  it("Ctrl-Kでgit-branchモードに切り替わる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.press(["ctrl", "k"]);
      await session.waitForText("git-branch>", { timeout: 5000 });

      expect(await getPromptName(session)).toMatchInlineSnapshot(
        `"git-branch>"`,
      );

      const items = await getItems(session);
      expect(items.some((i) => i.includes("main"))).toBe(true);
    } finally {
      session.close();
    }
  });

  it("fdモードでファイルを検索できる", async () => {
    const session = await launchFzfw(ctx);
    try {
      await session.press(["ctrl", "f"]);
      await session.waitForText("fd>", { timeout: 5000 });

      await session.type("Cargo.toml");
      await session.waitForText("Cargo.toml", { timeout: 5000 });

      const items = await getItems(session);
      expect(items.some((i) => i.includes("Cargo.toml"))).toBe(true);
    } finally {
      session.close();
    }
  });

  it("プレビューペインにコンテンツが表示される", async () => {
    const session = await launchFzfw(ctx);
    try {
      await sleep(500);
      const text = await session.text({ trimEnd: true });
      expect(text).toContain("No description");
    } finally {
      session.close();
    }
  });

  it("複数回モード切り替えしてもクラッシュしない", async () => {
    const session = await launchFzfw(ctx);
    try {
      const modes: { key: Key | Key[]; prompt: string }[] = [
        { key: ["ctrl", "f"], prompt: "fd>" },
        { key: "pagedown", prompt: "menu>" },
        { key: ["ctrl", "k"], prompt: "git-branch>" },
        { key: "pagedown", prompt: "menu>" },
        { key: ["ctrl", "g"], prompt: "livegrep>" },
        { key: "pagedown", prompt: "menu>" },
      ];

      for (const { key, prompt } of modes) {
        await session.press(key);
        await session.waitForText(prompt, { timeout: 5000 });
        const text = await session.text({ trimEnd: true });
        expect(text).toContain(prompt);
      }
    } finally {
      session.close();
    }
  });
});
