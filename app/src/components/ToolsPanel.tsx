import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  Copy,
  FolderOpen,
  Loader2,
  Play,
  RefreshCw,
  Search,
  Stethoscope,
  Terminal,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export type CheckItem = {
  id: string;
  ok: boolean;
  title: string;
  detail: string;
};

export type DoctorReport = {
  checks: CheckItem[];
  okCount: number;
  total: number;
  summary: string;
};

export type GrokSessionItem = {
  id: string;
  cwd: string;
  cwdLabel: string;
  path: string;
  updatedAt: number;
  lastPrompt?: string | null;
  title?: string | null;
  modelId?: string | null;
  sessionCount: number;
  promptCount: number;
  pathExists: boolean;
};

type Props = {
  modelId?: string | null;
  configPath?: string | null;
  onStatus?: (msg: string) => void;
};

function formatRelative(ms: number) {
  if (!ms) return "";
  const diff = Date.now() - ms;
  if (diff < 60_000) return "刚刚";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  if (diff < 86_400_000 * 7) return `${Math.floor(diff / 86_400_000)} 天前`;
  try {
    return new Date(ms).toLocaleDateString();
  } catch {
    return "";
  }
}

export function ToolsPanel({ modelId, configPath, onStatus }: Props) {
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [sessions, setSessions] = useState<GrokSessionItem[]>([]);
  const [cwd, setCwd] = useState("~");
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<GrokSessionItem | null>(null);
  const [deleting, setDeleting] = useState(false);

  const model = modelId?.trim() || undefined;

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [d, s] = await Promise.all([
        invoke<DoctorReport>("doctor", { configPath: configPath ?? null }),
        invoke<GrokSessionItem[]>("list_sessions", { limit: 30 }),
      ]);
      setDoctor(d);
      setSessions(s);
      setCwd((prev) => {
        if (prev !== "~") return prev;
        return s.find((x) => x.pathExists)?.cwd ?? s[0]?.cwd ?? prev;
      });
    } catch (err) {
      onStatus?.(`工具面板加载失败：${String(err)}`);
    } finally {
      setLoading(false);
    }
  }, [configPath, onStatus]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runAction = async (key: string, label: string, fn: () => Promise<string>) => {
    setBusyKey(key);
    try {
      const msg = await fn();
      onStatus?.(msg);
    } catch (err) {
      onStatus?.(`${label}失败：${String(err)}`);
    } finally {
      setBusyKey(null);
    }
  };

  const copyText = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
      onStatus?.("已复制路径");
    } catch {
      onStatus?.("复制失败");
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      // Prefer storage path under ~/.grok/sessions (safer + exact).
      const msg = await invoke<string>("delete_session", {
        path: deleteTarget.path || deleteTarget.cwd,
      });
      setSessions((prev) => prev.filter((x) => x.path !== deleteTarget.path));
      if (cwd === deleteTarget.cwd) {
        setCwd("~");
      }
      setDeleteTarget(null);
      onStatus?.(msg);
    } catch (err) {
      onStatus?.(`删除失败：${String(err)}`);
    } finally {
      setDeleting(false);
    }
  };

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      const hay = [
        s.cwdLabel,
        s.cwd,
        s.title ?? "",
        s.lastPrompt ?? "",
        s.modelId ?? "",
      ]
        .join(" ")
        .toLowerCase();
      return hay.includes(q);
    });
  }, [sessions, query]);

  return (
    <div className="mx-auto max-w-5xl space-y-4 px-5 pb-8 pt-2">
      <div className="flex items-center justify-between gap-3">
        <h2 className="text-sm font-medium">工具箱</h2>
        <Button variant="ghost" size="sm" disabled={loading} onClick={() => void refresh()}>
          {loading ? <Loader2 className="animate-spin" /> : <RefreshCw />}
          刷新
        </Button>
      </div>

      {/* Quick launch */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-xs font-medium text-muted-foreground">快捷启动</h3>
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
          <Input
            value={cwd}
            onChange={(e) => setCwd(e.target.value)}
            placeholder="工作目录，如 ~/Project/xxx"
            className="sm:flex-1"
          />
          <div className="flex flex-wrap gap-1.5">
            <Button
              size="sm"
              disabled={busyKey === "launch"}
              onClick={() =>
                void runAction("launch", "启动", () =>
                  invoke<string>("launch_grok", {
                    cwd,
                    model: model ?? null,
                  }),
                )
              }
            >
              {busyKey === "launch" ? <Loader2 className="animate-spin" /> : <Terminal />}
              打开 Grok
            </Button>
            <Button
              size="sm"
              variant="secondary"
              disabled={busyKey === "resume"}
              onClick={() =>
                void runAction("resume", "继续会话", () =>
                  invoke<string>("resume_session", { cwd }),
                )
              }
            >
              {busyKey === "resume" ? <Loader2 className="animate-spin" /> : <Play />}
              继续会话
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                void runAction("cfg", "打开配置", () =>
                  invoke<string>("open_config_file", {
                    configPath: configPath ?? null,
                  }),
                )
              }
            >
              <FolderOpen />
              配置
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                void runAction("dir", "打开目录", () => invoke<string>("open_config_dir"))
              }
            >
              <FolderOpen />
              ~/.grok
            </Button>
          </div>
        </div>
        {model ? (
          <p className="text-xs text-muted-foreground">
            启动模型 <code className="font-mono">{model}</code>
          </p>
        ) : null}
      </section>

      {/* Sessions — primary */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 className="text-xs font-medium text-muted-foreground">
            最近会话
            {sessions.length > 0 ? (
              <span className="ml-1.5 font-normal">· {sessions.length}</span>
            ) : null}
          </h3>
          <div className="relative w-full sm:w-56">
            <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="搜索路径 / 标题 / 模型"
              className="h-8 pl-7 text-xs"
            />
          </div>
        </div>

        {filtered.length === 0 ? (
          <p className="py-6 text-center text-xs text-muted-foreground">
            {sessions.length === 0 ? "暂无会话记录" : "没有匹配的会话"}
          </p>
        ) : (
          <ul className="max-h-[28rem] space-y-2 overflow-auto">
            {filtered.map((s) => {
              const selected = cwd === s.cwd;
              const busy =
                busyKey === `r:${s.cwd}` ||
                busyKey === `n:${s.cwd}` ||
                busyKey === `d:${s.path}`;
              return (
                <li
                  key={`${s.cwd}-${s.id}`}
                  className={cn(
                    "rounded-lg border px-3 py-2.5 transition-colors",
                    selected
                      ? "border-foreground/30 bg-muted/40"
                      : "border-border hover:border-foreground/20",
                    !s.pathExists && "opacity-70",
                  )}
                >
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <button
                      type="button"
                      className="min-w-0 flex-1 text-left"
                      onClick={() => setCwd(s.cwd)}
                      title="设为工作目录"
                    >
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
                        <span className="truncate text-sm font-medium">{s.cwdLabel}</span>
                        {!s.pathExists ? (
                          <span className="rounded border border-border px-1 py-px text-[10px] text-muted-foreground">
                            目录不存在
                          </span>
                        ) : null}
                      </div>
                      {(s.title || s.lastPrompt) && (
                        <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
                          {s.title || s.lastPrompt}
                        </p>
                      )}
                      <div className="mt-1.5 flex flex-wrap gap-x-2.5 gap-y-0.5 text-[11px] text-muted-foreground">
                        <span>{formatRelative(s.updatedAt)}</span>
                        {s.modelId ? (
                          <span className="font-mono text-foreground/80">{s.modelId}</span>
                        ) : null}
                        {s.sessionCount > 0 ? <span>{s.sessionCount} 次会话</span> : null}
                        {s.promptCount > 0 ? <span>{s.promptCount} 条提问</span> : null}
                      </div>
                    </button>

                    <div className="flex shrink-0 flex-wrap gap-1">
                      <Button
                        size="sm"
                        className="h-7"
                        disabled={!s.pathExists || busy}
                        onClick={() => {
                          setCwd(s.cwd);
                          void runAction(`r:${s.cwd}`, "继续会话", () =>
                            invoke<string>("resume_session", { cwd: s.cwd }),
                          );
                        }}
                      >
                        {busyKey === `r:${s.cwd}` ? (
                          <Loader2 className="animate-spin" />
                        ) : (
                          <Play />
                        )}
                        继续
                      </Button>
                      <Button
                        size="sm"
                        variant="secondary"
                        className="h-7"
                        disabled={!s.pathExists || busy}
                        onClick={() => {
                          setCwd(s.cwd);
                          void runAction(`n:${s.cwd}`, "打开 Grok", () =>
                            invoke<string>("launch_grok", {
                              cwd: s.cwd,
                              model: model ?? null,
                            }),
                          );
                        }}
                      >
                        {busyKey === `n:${s.cwd}` ? (
                          <Loader2 className="animate-spin" />
                        ) : (
                          <Terminal />
                        )}
                        新建
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2"
                        disabled={!s.pathExists}
                        title="在 Finder 中打开"
                        onClick={() =>
                          void runAction(`o:${s.cwd}`, "打开目录", () =>
                            invoke<string>("open_path", { path: s.cwd }),
                          )
                        }
                      >
                        <FolderOpen />
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2"
                        title="复制路径"
                        onClick={() => void copyText(s.cwd)}
                      >
                        {copied ? <Check /> : <Copy />}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 px-2 text-muted-foreground hover:text-destructive"
                        title="删除会话记录"
                        disabled={busy || deleting}
                        onClick={() => setDeleteTarget(s)}
                      >
                        <Trash2 />
                      </Button>
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <Dialog
        open={!!deleteTarget}
        onOpenChange={(open) => {
          if (!open && !deleting) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除会话记录？</DialogTitle>
            <DialogDescription>
              将删除「{deleteTarget?.cwdLabel}」在 ~/.grok/sessions
              下的全部会话数据（含历史提问），项目源码不会动。此操作不可撤销。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="secondary"
              disabled={deleting}
              onClick={() => setDeleteTarget(null)}
            >
              取消
            </Button>
            <Button
              variant="destructive"
              disabled={deleting}
              onClick={() => void confirmDelete()}
            >
              {deleting ? <Loader2 className="animate-spin" /> : <Trash2 />}
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Doctor */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <div className="flex items-center justify-between gap-2">
          <h3 className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
            <Stethoscope className="h-3.5 w-3.5" />
            环境体检
          </h3>
          {doctor ? (
            <span className="text-xs text-muted-foreground">
              {doctor.okCount}/{doctor.total}
            </span>
          ) : null}
        </div>
        {doctor ? (
          <>
            <p className="text-xs text-foreground">{doctor.summary}</p>
            <ul className="max-h-56 space-y-1.5 overflow-auto">
              {doctor.checks.map((c) => (
                <li
                  key={c.id}
                  className="rounded-md border border-border px-2.5 py-2 text-xs"
                >
                  <div className="flex items-start gap-2">
                    <span
                      className={cn(
                        "mt-1 inline-block h-1.5 w-1.5 shrink-0 rounded-full",
                        c.ok ? "bg-foreground" : "bg-destructive",
                      )}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="font-medium">{c.title}</div>
                      <div className="truncate text-muted-foreground" title={c.detail}>
                        {c.detail}
                      </div>
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          </>
        ) : (
          <p className="text-xs text-muted-foreground">点击刷新加载体检结果</p>
        )}
      </section>
    </div>
  );
}
