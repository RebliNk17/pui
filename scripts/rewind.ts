// Explicit user-initiated session navigation and bounded file checkpoints.
// Never rerun a prompt or synthesize an inverse patch. Pi owns the session tree.
import { readFile, writeFile, mkdir, realpath, lstat, rename, unlink, chmod } from "node:fs/promises";
import { resolve, relative, dirname, isAbsolute, join } from "node:path";
import { randomUUID, createHash } from "node:crypto";
import { pathToFileURL } from "node:url";

const LIMIT = 4 * 1024 * 1024, TOTAL = 16 * 1024 * 1024;
type FileState = { data: string; mode: number } | null;
type Change = { path: string; before: FileState; after: FileState };
type Checkpoint = { version: number; cwd: string; userId: string; changes: Change[]; reason?: string };
const digest = (data: string) => createHash("sha256").update(data).digest("hex");
const same = (a: FileState, b: FileState) => a === null ? b === null : b !== null && a.data === b.data && a.mode === b.mode;
const inspection = new Set(["read", "grep", "find", "ls", "tool_search", "pui_agents", "pui_ask_questions"]);

export async function workspacePath(cwd: string, input: unknown) {
  if (typeof input !== "string" || !input || input !== input.trim() || /^[~@]/.test(input) || /[\x00-\x1f]/.test(input)) throw Error("Unsupported checkpoint path");
  const root = await realpath(cwd), path = resolve(root, input), rel = relative(root, path);
  if (!rel || isAbsolute(rel) || rel.split(/[\\/]/).some(p => p === ".." || [".git", ".pi", ".ssh", ".aws", "node_modules"].includes(p) || /^\.env(?:\.|$)/.test(p))) throw Error("Protected or outside-workspace path");
  // Refuse symlinks at every component, including the destination, rather than
  // restoring through a link whose meaning might have changed since capture.
  let part = root;
  for (const segment of rel.split(/[\\/]/)) {
    part = join(part, segment);
    try { if ((await lstat(part)).isSymbolicLink()) throw Error("Symlink checkpoint path"); }
    catch (e: any) { if (e.code !== "ENOENT") throw e; }
  }
  return { root, path, rel };
}
async function readState(path: string): Promise<FileState> {
  try {
    const stat = await lstat(path);
    if (!stat.isFile() || stat.size > LIMIT || stat.isSymbolicLink()) throw Error("Checkpoint supports regular files up to 4 MiB only");
    const bytes = await readFile(path);
    if (bytes.length > LIMIT) throw Error("Checkpoint file exceeds size limit");
    return { data: bytes.toString("base64"), mode: stat.mode & 0o777 };
  } catch (e: any) { if (e.code === "ENOENT") return null; throw e; }
}
async function replace(path: string, state: FileState) {
  if (state === null) { try { await unlink(path); } catch (e: any) { if (e.code !== "ENOENT") throw e; } return; }
  await mkdir(dirname(path), { recursive: true });
  const temp = `${path}.pui-restore-${randomUUID()}`;
  await writeFile(temp, Buffer.from(state.data, "base64"), { flag: "wx", mode: state.mode });
  await chmod(temp, state.mode);
  await rename(temp, path);
}
async function privateDir(directory: string) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const stat = await lstat(directory);
  if (stat.isSymbolicLink() || !stat.isDirectory()) throw Error("Unsafe checkpoint directory");
  await chmod(directory, 0o700);
}
// Preflight every file before touching any. Keep a private recovery manifest;
// conflicts never silently overwrite work done after the recorded edits.
export async function restoreCheckpoints(cwd: string, checkpoints: Checkpoint[], directory: string) {
  const merged = new Map<string, Change>();
  let changes = 0, bytes = 0;
  for (const cp of checkpoints) {
    if (cp.version !== 1 || cp.cwd !== await realpath(cwd) || cp.reason) throw Error("This range has no complete file checkpoint");
    for (const change of cp.changes) {
      bytes += (change.before?.data.length ?? 0) + (change.after?.data.length ?? 0);
      if (++changes > 2048 || bytes > TOTAL * 4) throw Error("Checkpoint range exceeds safe restore limits");
      const old = merged.get(change.path);
      if (old && !same(old.after, change.before)) throw Error("File changed outside tracked edits between checkpoints");
      merged.set(change.path, { ...change, before: old ? old.before : change.before });
    }
  }
  const plans = [];
  for (const change of merged.values()) {
    const { path } = await workspacePath(cwd, change.path);
    if (same(change.before, change.after)) continue;
    if (!same(await readState(path), change.after)) throw Error(`File conflict: ${change.path}. No files were restored.`);
    plans.push({ ...change, absolute: path });
  }
  if (!plans.length) return { files: 0 };
  await privateDir(directory);
  const recovery = join(directory, `recovery-${randomUUID()}.json`);
  await writeFile(recovery, JSON.stringify({ version: 1, cwd, files: plans.map(p => ({ path: p.path, state: p.after })) }), { flag: "wx", mode: 0o600 });
  const done = [];
  try {
    for (const plan of plans) {
      await workspacePath(cwd, plan.path);
      if (!same(await readState(plan.absolute), plan.after)) throw Error(`File changed during restore: ${plan.path}`);
      await replace(plan.absolute, plan.before); done.push(plan);
    }
  } catch {
    for (const plan of done.reverse()) {
      try {
        await workspacePath(cwd, plan.path);
        if (same(await readState(plan.absolute), plan.before)) await replace(plan.absolute, plan.after);
      } catch { /* Recovery manifest remains available; never overwrite a new edit. */ }
    }
    throw Error(`Restore stopped. Recovery copy: ${recovery}. Check files before continuing.`);
  }
  return { files: plans.length, recovery };
}

// Pui already chose whether to restore its bounded checkpoints. Some installed
// extensions (pi-rewind) ask the same question from session_before_tree. Keep
// their files untouched, only for this exact navigation, and leave all other
// dialogs/extension hooks intact. Never auto-select a restoring option.
export async function navigateKeepingFiles(ctx: any, targetId: string) {
  const ui = ctx.ui, select = ui.select;
  const choices = new Set(["Keep current files", "Restore files to that point", "↩ Undo last rewind", "Cancel navigation"]);
  ui.select = (title: string, options: string[], ...args: any[]) => {
    if (title === "Restore Options" && Array.isArray(options)
      && options[0] === "Keep current files" && options.includes("Cancel navigation")
      && options.every(option => choices.has(option))) {
      return Promise.resolve("Keep current files");
    }
    return select.call(ui, title, options, ...args);
  };
  try { return await ctx.navigateTree(targetId, { summarize: false }); }
  finally { ui.select = select; }
}

export function installRewind(pi: any, options: any = {}) {
  let current: { timestamp: number; cp: Checkpoint } | undefined;
  let outstanding = new Map<string, string>();
  const directory = options.directory ?? process.env.PUI_CHECKPOINT_DIR;
  const blocked = (reason: string) => { if (current) current.cp.reason ||= reason; };
  const branch = (ctx: any) => ctx.sessionManager?.getBranch?.() ?? [];
  const users = (ctx: any) => branch(ctx).filter((e: any) => e.type === "message" && e.message.role === "user");
  const refs = (ctx: any) => new Map(branch(ctx).filter((e: any) => e.type === "custom" && e.customType === "pui-checkpoint").map((e: any) => [e.data.userId, e.data]));
  async function checkpoint(ref: any, ctx: any) {
    if (!directory || !ref || !/^[a-f0-9]{64}$/.test(ref.key)) throw Error("No file checkpoint captured for this message");
    const file = join(directory, `${ref.key}.json`);
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.size > TOTAL * 3) throw Error("Invalid file checkpoint");
    const raw = await readFile(file, "utf8");
    if (raw.length > TOTAL * 3 || digest(raw) !== ref.key) throw Error("Invalid file checkpoint");
    const cp = JSON.parse(raw);
    if (cp.cwd !== await realpath(ctx.cwd) || cp.userId !== ref.userId) throw Error("Checkpoint belongs to another workspace/message");
    return cp as Checkpoint;
  }
  function points(ctx: any) {
    const entries = branch(ctx).filter((e: any) => e.type === "message" && (e.message.role === "user" || (e.message.role === "assistant" && e.message.stopReason === "stop" && !e.message.errorMessage && !e.message.content?.some?.((b: any) => b.type === "toolCall")))), references = refs(ctx);
    let reason = branch(ctx).some((e: any) => e.type === "custom" && ["pui-code-restored", "pui-untracked-effect"].includes(e.customType)) ? "Files were independently restored or changed outside captured edits on this branch." : options.readonly ? "Read-only workers cannot restore files." : "", textBudget = 256000, fileChanges: number | null = 0;
    return entries.slice(-200).reverse().map((entry: any) => {
      const ref: any = references.get(entry.id);
      if (entry.message.role === "user") {
        reason ||= !ref ? "No file checkpoint was captured for this message." : ref.reason ?? "";
        fileChanges = fileChanges === null || !Number.isSafeInteger(ref?.fileChanges) || ref.fileChanges < 0
          ? null : fileChanges + ref.fileChanges;
      }
      const content = entry.message.content;
      const text = typeof content === "string" ? content : (content ?? []).filter((b: any) => b.type === "text").map((b: any) => b.text).join("\n\n");
      textBudget -= text.length;
      return { entryId: entry.id, role: entry.message.role, text: text.length <= 48000 && textBudget >= 0 ? text : "", timestamp: entry.message.timestamp ?? 0, imageCount: Array.isArray(content) ? content.filter((b: any) => b.type === "image").length : 0, codeReason: reason || null, fileChanges: reason ? null : fileChanges };
    }).reverse();
  }
  const status = (ctx: any) => ctx.ui.setStatus("pui-history", JSON.stringify({ sessionFile: ctx.sessionManager?.getSessionFile?.(), points: points(ctx) }));
  async function finish(ctx: any) {
    if (!current) return;
    const pending = current; current = undefined;
    const matches = users(ctx).filter((e: any) => e.message.timestamp === pending.timestamp);
    if (matches.length !== 1 || !directory) { status(ctx); return; }
    pending.cp.userId = matches[0].id;
    if (outstanding.size) pending.cp.reason ||= "An edit did not complete with a captured result.";
    outstanding.clear();
    try {
      if (pending.cp.reason) pending.cp.changes = [];
      await privateDir(directory);
      const raw = JSON.stringify(pending.cp), key = digest(raw);
      try { await writeFile(join(directory, `${key}.json`), raw, { flag: "wx", mode: 0o600 }); }
      catch (e: any) { if (e.code !== "EEXIST") throw e; }
      pi.appendEntry("pui-checkpoint", { userId: pending.cp.userId, key, reason: pending.cp.reason ?? null,
        fileChanges: pending.cp.reason ? null : pending.cp.changes.filter(c => !same(c.before, c.after)).length });
    } catch { /* Missing/unwritable checkpoint is explicitly unavailable. */ }
    status(ctx);
  }
  pi.on("session_start", (_e: any, ctx: any) => { current = undefined; outstanding.clear(); status(ctx); });
  pi.on("message_start", async (e: any, ctx: any) => {
    if (e.message?.role !== "user" || !directory) return;
    await finish(ctx);
    current = { timestamp: e.message.timestamp, cp: { version: 1, cwd: await realpath(ctx.cwd), userId: "", changes: [] } };
  });
  pi.on("tool_call", async (e: any, ctx: any) => {
    if (!current || current.cp.reason) return;
    if (inspection.has(e.toolName) || e.toolName === "codemode") return;
    if (!["edit", "write"].includes(e.toolName)) { blocked(`Code restore unavailable: ${e.toolName} may have untracked effects.`); return; }
    try {
      const { path, rel } = await workspacePath(ctx.cwd, e.input?.path);
      if ([...outstanding.values()].includes(rel)) throw Error("Concurrent edits to one file");
      let change = current.cp.changes.find(c => c.path === rel);
      const before = await readState(path);
      if (!change) { change = { path: rel, before, after: before }; current.cp.changes.push(change); }
      else if (!same(before, change.after)) blocked("File was changed outside tracked edits.");
      if (current.cp.changes.length > 128 || current.cp.changes.reduce((n,c) => n + (c.before?.data.length ?? 0) + (c.after?.data.length ?? 0),0) > TOTAL * 1.4) throw Error("Checkpoint size limit exceeded");
      outstanding.set(e.toolCallId, rel);
    } catch { blocked("An edited file could not be safely checkpointed."); }
  });
  pi.on("tool_result", async (e: any, ctx: any) => {
    const rel = outstanding.get(e.toolCallId); if (!rel || !current) return;
    outstanding.delete(e.toolCallId);
    try {
      const { path } = await workspacePath(ctx.cwd, rel); current.cp.changes.find(c => c.path === rel)!.after = await readState(path);
      if (current.cp.changes.reduce((n,c) => n + (c.before?.data.length ?? 0) + (c.after?.data.length ?? 0), 0) > TOTAL * 1.4) blocked("Checkpoint size limit exceeded.");
    }
    catch { blocked("An edited file could not be captured after the tool."); }
  });
  pi.on("user_bash", (_e: any, ctx: any) => {
    if (current) blocked("Shell execution may have untracked file effects.");
    else { pi.appendEntry("pui-untracked-effect", { reason: "Shell execution occurred outside captured edits." }); status(ctx); }
  });
  pi.on("agent_settled", async (_e: any, ctx: any) => { await finish(ctx); status(ctx); });
  pi.on("session_tree", (_e: any, ctx: any) => { current = undefined; outstanding.clear(); status(ctx); });
  pi.registerCommand("pui-history", {
    description: "Explicit Pui rewind/fork, never runs the selected prompt",
    handler: async (args: string, ctx: any) => {
      let request: any, restored: any;
      const result = (data: any) => ctx.ui.setStatus("pui-history-result", JSON.stringify({ id: request?.id, ...data }));
      try {
        request = JSON.parse(args);
        if (!ctx.isIdle() || ctx.hasPendingMessages()) throw Error("Stop or finish the current run and clear queued messages before rewinding.");
        if (!["rewind", "edit", "fork"].includes(request.operation) || !["messages", "both", "code"].includes(request.mode) || typeof request.id !== "string") throw Error("Invalid history operation");
        const entries = users(ctx), index = entries.findIndex((e: any) => e.id === request.entryId);
        const target = branch(ctx).find((e: any) => e.id === request.entryId && e.type === "message");
        if (!target) throw Error("This message is not on the active branch. Refresh the chat.");
        if (request.operation === "fork") {
          if (options.readonly) throw Error("Read-only workers cannot create writable fork sessions");
          if (request.mode !== "messages") throw Error("Fork preserves the original workspace; only messages are branched.");
          if (target.message.role !== "assistant" || target.message.stopReason !== "stop" || target.message.errorMessage || target.message.content?.some?.((b: any) => b.type === "toolCall")) throw Error("Fork must start after a completed assistant response.");
        } else if (index < 0) throw Error("Edit and rewind must target a user prompt.");
        if (request.mode !== "messages" && branch(ctx).some((e: any) => e.type === "custom" && ["pui-code-restored", "pui-untracked-effect"].includes(e.customType))) throw Error("This branch has independently restored files or untracked effects; code restore is unavailable.");
        if (request.mode !== "messages") {
          if (options.readonly) throw Error("Read-only workers cannot restore files");
          const references = refs(ctx), cps = []; let size = 0;
          if (entries.length - index > 200) throw Error("Restore at most 200 messages at a time");
          for (const entry of entries.slice(index)) {
            const cp = await checkpoint(references.get(entry.id), ctx);
            size += JSON.stringify(cp).length;
            if (size > TOTAL * 4) throw Error("Checkpoint range exceeds safe restore limits");
            cps.push(cp);
          }
          restored = await restoreCheckpoints(ctx.cwd, cps, directory);
        }
        if (request.operation === "fork") {
          if (!process.env.PUI_PI_SDK) throw Error("Pi SDK unavailable for session branching");
          const { SessionManager } = await import(pathToFileURL(process.env.PUI_PI_SDK).href);
          const source = ctx.sessionManager.getSessionFile();
          const manager = SessionManager.open(source);
          const path = manager.createBranchedSession(target.id);
          // Ensure a new session file exists for its separate RPC owner.
          // Keep Pi's canonical entries; never invent or replay conversation.
          try { await lstat(path); } catch (e: any) {
            if (e.code !== "ENOENT") throw e;
            await writeFile(path, [manager.getHeader(), ...manager.getEntries()].map(e => JSON.stringify(e)).join("\n") + "\n", { flag: "wx", mode: 0o600 });
          }
          result({ success: true, path }); return;
        }
        if (request.mode !== "code") {
          const moved = await navigateKeepingFiles(ctx, target.id);
          if (moved.cancelled) throw Error("Navigation cancelled.");
          if (branch(ctx).some((e: any) => e.id === target.id)) throw Error("Pi did not move the active branch; check runtime compatibility.");
          // Persist the leaf selection without deleting the abandoned branch.
          pi.appendEntry("pui-rewind", { target: target.id, mode: request.mode });
        } else {
          pi.appendEntry("pui-code-restored", { target: target.id });
        }
        result({ success: true, files: restored?.files ?? 0, recovery: restored?.recovery }); status(ctx);
      } catch (e: any) {
        // Errors are app-generated; no provider/auth messages or arbitrary stderr.
        const restoredNote = restored?.files ? ` ${restored.files} file(s) were restored, but conversation navigation failed. Recovery copy: ${restored.recovery ?? "unavailable"}` : "";
        result({ success: false, error: (String(e.message ?? "History operation failed") + restoredNote).slice(0, 1000) });
      }
    },
  });
}
