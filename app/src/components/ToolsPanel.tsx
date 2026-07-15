import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  ChevronRight,
  Copy,
  FolderOpen,
  Loader2,
  Play,
  RefreshCw,
  Send,
  Stethoscope,
  Terminal,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
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
};

export type QuickAskResult = {
  ok: boolean;
  command: string;
  output: string;
  elapsedMs: number;
};

type Props = {
  modelId?: string | null;
  configPath?: string | null;
  onStatus?: (msg: string) => void;
};

function formatTime(ms: number) {
  if (!ms) return "";
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return "";
  }
}

export function ToolsPanel({ modelId, configPath, onStatus }: Props) {
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [sessions, setSessions] = useState<GrokSessionItem[]>([]);
  const [recipes, setRecipes] = useState<[string, string][]>([]);
  const [cwd, setCwd] = useState("~");
  const [prompt, setPrompt] = useState("");
  const [askResult, setAskResult] = useState<QuickAskResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [asking, setAsking] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  const model = modelId?.trim() || undefined;

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [d, s] = await Promise.all([
        invoke<DoctorReport>("doctor", { configPath: configPath ?? null }),
        invoke<GrokSessionItem[]>("list_sessions", { limit: 10 }),
      ]);
      setDoctor(d);
      setSessions(s);
      setCwd((prev) => {
        if (prev !== "~") return prev;
        return s[0]?.cwd ?? prev;
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

  // Keep recipe commands in sync when cwd/model changes.
  useEffect(() => {
    void (async () => {
      try {
        const r = await invoke<[string, string][]>("recipe_commands", {
          model: model ?? null,
          cwd,
        });
        setRecipes(r);
      } catch {
        /* ignore */
      }
    })();
  }, [cwd, model]);

  const runAction = async (label: string, fn: () => Promise<string>) => {
    try {
      const msg = await fn();
      onStatus?.(msg);
    } catch (err) {
      onStatus?.(`${label}失败：${String(err)}`);
    }
  };

  const handleAsk = async () => {
    if (!prompt.trim()) return;
    setAsking(true);
    setAskResult(null);
    try {
      const result = await invoke<QuickAskResult>("quick_ask", {
        prompt: prompt.trim(),
        model: model ?? null,
        cwd,
      });
      setAskResult(result);
      onStatus?.(
        result.ok
          ? `提问完成 · ${result.elapsedMs}ms`
          : `提问失败 · ${result.elapsedMs}ms`,
      );
    } catch (err) {
      onStatus?.(`提问失败：${String(err)}`);
    } finally {
      setAsking(false);
    }
  };

  const copyText = async (key: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(key);
      window.setTimeout(() => setCopied(null), 1200);
      onStatus?.("已复制到剪贴板");
    } catch {
      onStatus?.("复制失败");
    }
  };

  const tips = useMemo(
    () => [
      { q: "解释这段报错", hint: "把终端错误贴进提问框" },
      { q: "帮我写一个 commit message", hint: "先 git diff" },
      { q: "这个项目是干什么的？", hint: "在项目目录启动 Grok" },
    ],
    [],
  );

  return (
    <div className="mx-auto max-w-5xl space-y-6 px-5 pb-8 pt-2">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Grok 工具箱</h2>
          <p className="text-[11px] text-muted-foreground">
            不用记命令行 · 一键体检 / 启动 / 提问 / 继续会话
          </p>
        </div>
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
              onClick={() =>
                void runAction("启动", () =>
                  invoke<string>("launch_grok", {
                    cwd,
                    model: model ?? null,
                  }),
                )
              }
            >
              <Terminal />
              打开 Grok
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() =>
                void runAction("继续会话", () =>
                  invoke<string>("resume_session", { cwd }),
                )
              }
            >
              <Play />
              继续会话
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                void runAction("打开配置", () =>
                  invoke<string>("open_config_file", {
                    configPath: configPath ?? null,
                  }),
                )
              }
            >
              <FolderOpen />
              配置文件
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() =>
                void runAction("打开目录", () => invoke<string>("open_config_dir"))
              }
            >
              <FolderOpen />
              ~/.grok
            </Button>
          </div>
        </div>
        {model ? (
          <p className="text-[11px] text-muted-foreground">
            启动时使用模型 <code className="font-mono">{model}</code>
          </p>
        ) : null}
      </section>

      {/* Quick ask */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-xs font-medium text-muted-foreground">
          快捷提问（无需进入交互界面）
        </h3>
        <Textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="例如：用三句话解释这个项目在做什么"
          className="min-h-[88px]"
        />
        <div className="flex flex-wrap items-center gap-1.5">
          {tips.map((t) => (
            <button
              key={t.q}
              type="button"
              className="rounded-md border border-border px-2 py-1 text-[11px] text-muted-foreground hover:text-foreground"
              onClick={() => setPrompt(t.q)}
              title={t.hint}
            >
              {t.q}
            </button>
          ))}
          <div className="ml-auto">
            <Button size="sm" disabled={asking || !prompt.trim()} onClick={() => void handleAsk()}>
              {asking ? <Loader2 className="animate-spin" /> : <Send />}
              提问
            </Button>
          </div>
        </div>
        {askResult ? (
          <div className="space-y-1.5 rounded-md border border-border bg-background p-3">
            <div className="flex items-center justify-between gap-2 text-[11px] text-muted-foreground">
              <span className={cn(askResult.ok ? "text-foreground" : "text-destructive")}>
                {askResult.ok ? "成功" : "失败"} · {askResult.elapsedMs}ms
              </span>
              <button
                type="button"
                className="inline-flex items-center gap-1 hover:text-foreground"
                onClick={() => void copyText("ask", askResult.output)}
              >
                {copied === "ask" ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                复制回答
              </button>
            </div>
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed text-foreground">
              {askResult.output}
            </pre>
          </div>
        ) : null}
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        {/* Doctor */}
        <section className="space-y-3 rounded-lg border border-border bg-card p-4">
          <div className="flex items-center justify-between gap-2">
            <h3 className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
              <Stethoscope className="h-3.5 w-3.5" />
              环境体检
            </h3>
            {doctor ? (
              <span className="text-[11px] text-muted-foreground">
                {doctor.okCount}/{doctor.total}
              </span>
            ) : null}
          </div>
          {doctor ? (
            <>
              <p className="text-xs text-foreground">{doctor.summary}</p>
              <ul className="max-h-64 space-y-1.5 overflow-auto">
                {doctor.checks.map((c) => (
                  <li
                    key={c.id}
                    className="rounded-md border border-border px-2.5 py-2 text-xs"
                  >
                    <div className="flex items-start gap-2">
                      <span
                        className={cn(
                          "mt-0.5 inline-block h-1.5 w-1.5 shrink-0 rounded-full",
                          c.ok ? "bg-foreground" : "bg-destructive",
                        )}
                      />
                      <div className="min-w-0 flex-1">
                        <div className="font-medium">{c.title}</div>
                        <div className="truncate text-[11px] text-muted-foreground" title={c.detail}>
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

        {/* Sessions */}
        <section className="space-y-3 rounded-lg border border-border bg-card p-4">
          <h3 className="text-xs font-medium text-muted-foreground">最近会话</h3>
          {sessions.length === 0 ? (
            <p className="text-xs text-muted-foreground">暂无会话记录</p>
          ) : (
            <ul className="max-h-72 space-y-1.5 overflow-auto">
              {sessions.map((s) => (
                <li
                  key={`${s.cwd}-${s.id}`}
                  className="rounded-md border border-border px-2.5 py-2"
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs font-medium">{s.cwdLabel}</div>
                      <div className="text-[11px] text-muted-foreground">
                        {formatTime(s.updatedAt)}
                      </div>
                      {s.lastPrompt ? (
                        <div className="mt-1 line-clamp-2 text-[11px] text-muted-foreground">
                          {s.lastPrompt}
                        </div>
                      ) : null}
                    </div>
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-7 shrink-0 px-2"
                      onClick={() => {
                        setCwd(s.cwd);
                        void runAction("继续会话", () =>
                          invoke<string>("resume_session", { cwd: s.cwd }),
                        );
                      }}
                    >
                      继续
                      <ChevronRight className="h-3 w-3" />
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>

      {/* Recipes */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h3 className="text-xs font-medium text-muted-foreground">常用命令（可复制）</h3>
        <ul className="space-y-1.5">
          {recipes.map(([label, cmd]) => (
            <li
              key={label}
              className="flex items-center gap-2 rounded-md border border-border px-2.5 py-2"
            >
              <div className="min-w-0 flex-1">
                <div className="text-xs font-medium">{label}</div>
                <code className="block truncate font-mono text-[11px] text-muted-foreground">
                  {cmd}
                </code>
              </div>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 shrink-0"
                onClick={() => void copyText(label, cmd)}
              >
                {copied === label ? <Check /> : <Copy />}
              </Button>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
