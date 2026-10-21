// @vitest-environment node
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import childProcess from "node:child_process";
import { EventEmitter } from "node:events";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { createServer } from "vite";

// Exercise the installed middleware. Editor commands are intercepted, never run.
describe.runIf(process.platform === "win32")("Vite Windows editor boundary", () => {
  let root;
  let server;
  let baseUrl;
  const editor = new EventEmitter();

  beforeAll(async () => {
    root = await mkdtemp(join(tmpdir(), "whispertube-vite-security-"));
    await writeFile(join(root, "ordinary file.ts"), "export {};\n");
    await writeFile(join(root, "%WHISPERTUBE_EDITOR_TEST%.ts"), "export {};\n");
    server = await createServer({
      configFile: false,
      root,
      logLevel: "silent",
      server: { host: "127.0.0.1", port: 0, strictPort: false },
      optimizeDeps: { noDiscovery: true, include: [] },
    });
    await server.listen();
    const address = server.httpServer.address();
    if (!address || typeof address === "string") throw new Error("No Vite HTTP port");
    baseUrl = `http://127.0.0.1:${address.port}`;
  });

  afterEach(() => {
    editor.emit("exit", 0);
    vi.restoreAllMocks();
    vi.unstubAllEnvs();
  });

  afterAll(async () => {
    await server?.close();
    if (root && basename(root).startsWith("whispertube-vite-security-")) {
      await rm(root, { recursive: true, force: true });
    }
  });

  async function openEditor(file) {
    return fetch(`${baseUrl}/__open-in-editor?${new URLSearchParams({ file })}`);
  }

  function interceptEditor() {
    vi.stubEnv("LAUNCH_EDITOR", "code");
    vi.stubEnv("WHISPERTUBE_EDITOR_TEST", "harmless-fixture");
    vi.spyOn(console, "log").mockImplementation(() => undefined);
    return vi.spyOn(childProcess, "exec").mockImplementation(() => editor);
  }

  it("rejects percent expansion for an existing file and its file URL representation", async () => {
    const execute = interceptEditor();
    const file = join(root, "%WHISPERTUBE_EDITOR_TEST%.ts");
    const { pathToFileURL } = await import("node:url");
    await openEditor(file);
    await openEditor(pathToFileURL(file).href);
    expect(execute).not.toHaveBeenCalled();
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('Cannot launch an editor with an argument containing "%"'));
  });

  it("never carries a crafted directory/position suffix into the Windows shell", async () => {
    const execute = interceptEditor();
    await openEditor(`${join(root, "%WHISPERTUBE_EDITOR_TEST%")}\\..:1:1`);
    for (const [command] of execute.mock.calls) {
      expect(command).not.toContain("%WHISPERTUBE_EDITOR_TEST%");
      expect(command).not.toContain("\\..:1:1");
    }
  });

  it("keeps ordinary editor navigation and the HMR client working", async () => {
    const execute = interceptEditor();
    expect((await openEditor(`${join(root, "ordinary file.ts")}:2:1`)).status).toBe(200);
    expect(execute).toHaveBeenCalledOnce();
    expect(execute.mock.calls[0][0]).toContain("ordinary file.ts");
    const response = await fetch(`${baseUrl}/@vite/client`);
    expect(response.status).toBe(200);
    expect(await response.text()).toContain("WebSocket");
  });
});
