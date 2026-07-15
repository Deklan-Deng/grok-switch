import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Check,
  Copy,
  Download,
  Gauge,
  Loader2,
  MoreHorizontal,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { ThemeToggle } from "@/components/ThemeToggle";
import type { UsageError } from "@/lib/api";
import {
  callApi,
  checkHealth,
  getUsageSummary,
  lastHealth,
  runSpeedTest,
  type CommandResult,
  type CreateProviderInput,
  type HealthResult,
  type ProfilePatch,
  type SpeedTestResult,
  type TokenProfile,
  type UsageSummary,
} from "@/lib/api";
import { cn } from "@/lib/utils";
import {
  ConfigRawEditor,
  emptyForm,
  type EditorForm,
  profileToForm,
  ProviderEditor,
} from "@/components/ProviderEditor";
import { ToolsPanel } from "@/components/ToolsPanel";
import { UpdateChecker } from "@/components/UpdateChecker";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import logo from "@/assets/logo.png";

type View =
  | { kind: "list" }
  | { kind: "tools" }
  | { kind: "create" }
  | { kind: "edit"; id: string };

function parseNum(raw: string): number | null {
  const t = raw.trim();
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : null;
}

function formToPatch(id: string, form: EditorForm): ProfilePatch {
  return {
    id,
    name: form.name.trim(),
    modelId: form.modelId.trim(),
    apiModel: form.apiModel,
    modelAlias: form.modelAlias,
    description: form.description,
    baseUrl: form.baseUrl,
    envKey: form.envKey,
    apiBackend: form.apiBackend,
    contextWindow: parseNum(form.contextWindow) ?? 0,
    maxCompletionTokens: parseNum(form.maxCompletionTokens) ?? 0,
    setAsDefault: form.setAsDefault,
    configPath: form.configPath,
  };
}

function formToCreate(form: EditorForm, enable: boolean): CreateProviderInput {
  return {
    name: form.name.trim(),
    modelId: form.modelId.trim(), // ProviderEditor already resolves from name if empty
    apiModel: form.apiModel || null,
    modelAlias: form.modelAlias || null,
    description: form.description || null,
    baseUrl: form.baseUrl || null,
    envKey: form.envKey || null,
    apiBackend: form.apiBackend || null,
    contextWindow: parseNum(form.contextWindow),
    maxCompletionTokens: parseNum(form.maxCompletionTokens),
    configPath: form.configPath || null,
    setAsDefault: form.setAsDefault,
    token: form.token || null,
    enable,
  };
}

export default function App() {
  const [profiles, setProfiles] = useState<TokenProfile[]>([]);
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [defaultModelId, setDefaultModelId] = useState<string | null>(null);
  const [status, setStatus] = useState("就绪");
  const [busy, setBusy] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ kind: "list" });

  const [editForm, setEditForm] = useState<EditorForm>(emptyForm());
  const [configText, setConfigText] = useState("");
  const [configPath, setConfigPath] = useState<string | null>(null);

  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<TokenProfile | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  /** In-flight speed tests by profile id — several cards can run in parallel. */
  const [speedTestingIds, setSpeedTestingIds] = useState<Record<string, true>>({});
  const [enablingId, setEnablingId] = useState<string | null>(null);
  const [healthMap, setHealthMap] = useState<Record<string, HealthResult>>({});
  const [speedMap, setSpeedMap] = useState<Record<string, SpeedTestResult>>({});
  const [speedDialog, setSpeedDialog] = useState<SpeedTestResult | null>(null);
  const [usage, setUsage] = useState<UsageSummary | null>(null);
  const [usageLoading, setUsageLoading] = useState(false);

  const current = useMemo(
    () => profiles.find((p) => p.id === currentId) ?? null,
    [profiles, currentId],
  );

  const currentHealth = currentId ? healthMap[currentId] : undefined;

  const editTarget = useMemo(() => {
    if (view.kind !== "edit") return null;
    return profiles.find((p) => p.id === view.id) ?? null;
  }, [view, profiles]);

  const applyResult = useCallback((result: CommandResult) => {
    setProfiles(result.profiles ?? []);
    setCurrentId(result.currentId ?? null);
    setDefaultModelId(result.defaultModelId ?? null);
    setStatus(result.status ?? "就绪");
    setBusy(!!result.busy);
    if (typeof result.configText === "string") {
      setConfigText(result.configText);
    }
    if (result.configPath) {
      setConfigPath(result.configPath);
    }
  }, []);

  const run = useCallback(
    async (cmd: string, args?: Record<string, unknown>) => {
      try {
        const result = await callApi(cmd, args);
        applyResult(result);
        return result;
      } catch (err) {
        setStatus(`操作失败：${String(err)}`);
        return null;
      }
    },
    [applyResult],
  );

  const refreshUsage = useCallback(async (force = false) => {
    setUsageLoading(true);
    try {
      setUsage(await getUsageSummary(24, force));
    } catch (err) {
      setStatus(`用量读取失败：${String(err)}`);
    } finally {
      setUsageLoading(false);
    }
  }, []);

  const upsertHealth = useCallback((h: HealthResult) => {
    setHealthMap((prev) => ({ ...prev, [h.profileId]: h }));
  }, []);

  // Boot: list first, then defer probes so the first paint is always clickable.
  useEffect(() => {
    let cancelled = false;
    let deferred: number | undefined;
    void (async () => {
      try {
        const state = await callApi("get_state");
        if (cancelled) return;
        applyResult(state);

        // Cached health chip only (no network on open).
        try {
          const cached = await lastHealth();
          if (!cancelled && cached.length) {
            setHealthMap((prev) => {
              const next = { ...prev };
              for (const h of cached) next[h.profileId] = h;
              return next;
            });
          }
        } catch {
          /* ignore missing cache */
        }

        // After first paint + a short idle: usage + optional health in background.
        // Opening must never wait on third-party /models or multi-MB log scans.
        deferred = window.setTimeout(() => {
          if (cancelled) return;
          void refreshUsage(false);
          if (state.currentId) {
            void checkHealth(state.currentId)
              .then((h) => {
                if (!cancelled) upsertHealth(h);
              })
              .catch(() => {
                /* ignore boot health errors */
              });
          }
        }, 400);
      } catch (err) {
        if (!cancelled) {
          setBootError(String(err));
          setStatus(`初始化失败：${String(err)}`);
        }
      }
    })();
    return () => {
      cancelled = true;
      if (deferred !== undefined) window.clearTimeout(deferred);
    };
  }, [applyResult, refreshUsage, upsertHealth]);

  // Menu bar tray can switch providers / push status while window is hidden.
  useEffect(() => {
    const unsubs: Array<() => void> = [];
    void (async () => {
      unsubs.push(
        await listen<CommandResult>("app://state", (event) => {
          applyResult(event.payload);
        }),
      );
      unsubs.push(
        await listen<string>("app://status", (event) => {
          setStatus(event.payload);
        }),
      );
      unsubs.push(
        await listen<HealthResult>("app://health", (event) => {
          upsertHealth(event.payload);
          setStatus(
            event.payload.ok
              ? `「${event.payload.name}」${event.payload.title} · ${event.payload.detail}`
              : `「${event.payload.name}」${event.payload.title} · ${event.payload.detail} — ${event.payload.hint}`,
          );
        }),
      );
    })();
    return () => {
      for (const off of unsubs) off();
    };
  }, [applyResult, upsertHealth]);

  const openCreate = () => {
    setEditForm(emptyForm());
    setView({ kind: "create" });
  };

  const openEdit = async (provider: TokenProfile) => {
    const result = await run("load_token", { id: provider.id });
    setEditForm(profileToForm(provider, result?.token ?? ""));
    setView({ kind: "edit", id: provider.id });
    // refresh full config for the bottom editor
    await run("read_config_file", { configPath: provider.configPath });
  };

  const handleCreateSave = async (form: EditorForm) => {
    const result = await run("create_provider", {
      input: formToCreate(form, false),
    });
    if (result) setView({ kind: "list" });
  };

  const handleCreateEnable = async (form: EditorForm) => {
    const result = await run("create_provider", {
      input: formToCreate(form, true),
    });
    if (result) setView({ kind: "list" });
  };

  const handleEditSave = async (form: EditorForm) => {
    if (!editTarget) return;
    await run("update_profile", { patch: formToPatch(editTarget.id, form) });
    if (editTarget.id === currentId) {
      await run("apply_token", {
        id: editTarget.id,
        draftToken: form.token.trim() ? form.token : null,
      });
    } else if (form.token.trim()) {
      await run("save_token", { id: editTarget.id, token: form.token });
    }
    setView({ kind: "list" });
  };

  const handleEditEnable = async (form: EditorForm) => {
    if (!editTarget) return;
    await run("update_profile", { patch: formToPatch(editTarget.id, form) });
    await run("apply_token", {
      id: editTarget.id,
      draftToken: form.token.trim() ? form.token : null,
    });
    setView({ kind: "list" });
  };

  /** Let React paint loading state before a long invoke. */
  const paintThen = () =>
    new Promise<void>((resolve) => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    });

  const handleEnable = async (provider: TokenProfile) => {
    setEnablingId(provider.id);
    setStatus(`正在启用「${provider.name}」…`);
    await paintThen();
    try {
      // Switch should feel instant: write config only, probe health in the background.
      const result = await run("apply_token", {
        id: provider.id,
        draftToken: null,
      });
      if (!result) return;
      void (async () => {
        try {
          const h = await checkHealth(provider.id);
          upsertHealth(h);
          setStatus(
            h.ok
              ? `${result.status} · ${h.title} ${h.latencyMs != null ? `${h.latencyMs}ms` : ""}`.trim()
              : `${result.status} · ${h.title}：${h.hint}`,
          );
        } catch {
          /* keep apply status */
        }
      })();
    } finally {
      setEnablingId(null);
    }
  };

  const markSpeedTesting = (id: string, on: boolean) => {
    setSpeedTestingIds((prev) => {
      if (on) return { ...prev, [id]: true };
      const next = { ...prev };
      delete next[id];
      return next;
    });
  };

  const handleSpeedTest = async (provider: TokenProfile) => {
    // Same card already running — ignore. Other cards can still start in parallel.
    if (speedTestingIds[provider.id]) return;
    let startedAlone = true;
    let parallelCount = 1;
    setSpeedTestingIds((prev) => {
      startedAlone = Object.keys(prev).length === 0;
      const next = { ...prev, [provider.id]: true as const };
      parallelCount = Object.keys(next).length;
      return next;
    });
    setStatus(
      parallelCount > 1
        ? `测速并行中（${parallelCount}）· 已加入「${provider.name}」`
        : `正在测速「${provider.name}」…`,
    );
    await paintThen();
    try {
      const result = await runSpeedTest(provider.id);
      // Per-profile map: finishing A never wipes B's result.
      setSpeedMap((prev) => ({ ...prev, [result.profileId]: result }));
      // Auto-open detail only for a single interactive run. Parallel → stay on card.
      if (startedAlone) {
        setSpeedDialog(result);
      }
      setStatus(speedStatusLine(result));
    } catch (err) {
      setStatus(`「${provider.name}」测速失败：${String(err)}`);
    } finally {
      markSpeedTesting(provider.id, false);
    }
  };

  const showChrome = view.kind === "list" || view.kind === "tools";

  return (
    <div className="flex h-full flex-col">
      {showChrome ? (
        <>
          <header className="flex items-center justify-between gap-3 border-b border-border bg-card px-5 py-3">
            <div className="flex min-w-0 items-center gap-2.5">
              <img
                src={logo}
                alt="Grok Switch"
                className="h-7 w-7 shrink-0 rounded-md border border-border object-cover"
              />
              <div className="min-w-0">
                <div className="text-sm font-medium tracking-tight">Grok Switch</div>
              </div>
              <div className="ml-2 flex items-center rounded-md border border-border p-0.5 text-xs">
                <button
                  type="button"
                  className={cn(
                    "rounded px-2.5 py-1 transition-colors",
                    view.kind === "list"
                      ? "bg-foreground text-background"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                  onClick={() => setView({ kind: "list" })}
                >
                  供应商
                </button>
                <button
                  type="button"
                  className={cn(
                    "rounded px-2.5 py-1 transition-colors",
                    view.kind === "tools"
                      ? "bg-foreground text-background"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                  onClick={() => setView({ kind: "tools" })}
                >
                  工具箱
                </button>
              </div>
            </div>

            <div className="flex flex-wrap items-center gap-1.5">
              {view.kind === "list" ? (
                <>
                  <Button
                    variant="ghost"
                    size="icon"
                    className="h-7 w-7"
                    disabled={busy}
                    title="从 config.toml 导入"
                    aria-label="从 config.toml 导入"
                    onClick={() => setImportOpen(true)}
                  >
                    <Download />
                  </Button>
                  <Button size="sm" onClick={openCreate}>
                    <Plus />
                    添加
                  </Button>
                </>
              ) : null}
              <ThemeToggle />
            </div>
          </header>

          <StatusBar
            current={current}
            defaultModelId={defaultModelId}
            health={currentHealth}
            usage={usage}
            usageLoading={usageLoading}
            onRefreshUsage={() => void refreshUsage(true)}
          />
        </>
      ) : null}

      <ScrollArea className="flex-1">
        {view.kind === "list" ? (
          <div className="mx-auto max-w-5xl px-5 py-5">
            {bootError ? (
              <div className="mb-4 rounded-md border border-border px-3 py-2 text-sm text-destructive">
                {bootError}
              </div>
            ) : null}

            {profiles.length === 0 ? (
              <EmptyState onCreate={openCreate} onTools={() => setView({ kind: "tools" })} />
            ) : (
              <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3 lg:grid-cols-4">
                {profiles.map((provider) => (
                  <ProviderCard
                    key={provider.id}
                    provider={provider}
                    isCurrent={provider.id === currentId}
                    busy={busy}
                    enabling={enablingId === provider.id}
                    speedTesting={!!speedTestingIds[provider.id]}
                    health={healthMap[provider.id]}
                    speed={speedMap[provider.id]}
                    onEnable={() => void handleEnable(provider)}
                    onSpeedTest={() => void handleSpeedTest(provider)}
                    onShowSpeed={() => {
                      const s = speedMap[provider.id];
                      if (s) setSpeedDialog(s);
                    }}
                    onEdit={() => void openEdit(provider)}
                    onDelete={() => {
                      setDeleteTarget(provider);
                      setDeleteOpen(true);
                    }}
                  />
                ))}
                <button
                  type="button"
                  onClick={openCreate}
                  className="aspect-square rounded-lg border border-dashed border-border text-muted-foreground transition-colors hover:border-foreground/30 hover:text-foreground"
                >
                  <div className="flex h-full flex-col items-center justify-center gap-2 p-4">
                    <Plus className="h-5 w-5" />
                    <span className="text-xs">添加</span>
                  </div>
                </button>
              </div>
            )}
          </div>
        ) : view.kind === "tools" ? (
          <ToolsPanel
            modelId={current?.modelId}
            configPath={current?.configPath ?? profiles[0]?.configPath}
            onStatus={setStatus}
          />
        ) : view.kind === "create" ? (
          <>
            <ProviderEditor
              mode="create"
              title="添加供应商"
              initial={editForm}
              busy={busy}
              onBack={() => setView({ kind: "list" })}
              onSave={handleCreateSave}
              onEnable={handleCreateEnable}
            />
            <ConfigRawEditor
              path={configPath}
              value={configText}
              busy={busy}
              onChange={setConfigText}
              onReload={() => void run("read_config_file", {})}
              onSave={() =>
                void run("write_config_file", {
                  configPath,
                  content: configText,
                })
              }
            />
          </>
        ) : (
          <>
            <ProviderEditor
              mode="edit"
              title={editTarget ? `编辑 · ${editTarget.name}` : "编辑供应商"}
              initial={editForm}
              busy={busy}
              isCurrent={editTarget?.id === currentId}
              onBack={() => setView({ kind: "list" })}
              onSave={handleEditSave}
              onEnable={handleEditEnable}
            />
            <ConfigRawEditor
              path={configPath}
              value={configText}
              busy={busy}
              onChange={setConfigText}
              onReload={() =>
                void run("read_config_file", {
                  configPath: editTarget?.configPath,
                })
              }
              onSave={() =>
                void run("write_config_file", {
                  configPath: editTarget?.configPath ?? configPath,
                  content: configText,
                })
              }
            />
          </>
        )}
      </ScrollArea>

      <footer className="flex items-center justify-between gap-3 border-t border-border px-5 py-2 text-xs text-muted-foreground">
        <span className="min-w-0 truncate">{status}</span>
        <div className="flex shrink-0 items-center gap-2">
          <UpdateChecker onStatus={setStatus} />
          <a
            className="hover:text-foreground"
            href="https://x.ai/cli"
            target="_blank"
            rel="noreferrer"
          >
            docs
          </a>
        </div>
      </footer>

      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>删除供应商？</DialogTitle>
            <DialogDescription>
              “{deleteTarget?.name}”及其本地保存的 Token 将被删除。当前已启用的供应商不能删除。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setDeleteOpen(false)}>
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                if (!deleteTarget) return;
                void run("remove_profile", { id: deleteTarget.id }).then(() =>
                  setDeleteOpen(false),
                );
              }}
            >
              删除
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={importOpen} onOpenChange={setImportOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>从 config.toml 导入？</DialogTitle>
            <DialogDescription>
              读取本机 Grok 配置（默认 ~/.grok/config.toml，Windows 为
              %USERPROFILE%\.grok\config.toml）中尚未在列表里的模型段，添加为供应商卡片。不会导入
              API Key，已有同名段名会跳过。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="secondary" onClick={() => setImportOpen(false)}>
              取消
            </Button>
            <Button
              disabled={busy}
              onClick={() => {
                void run("import_from_config", {}).then(() => setImportOpen(false));
              }}
            >
              导入
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={!!speedDialog}
        onOpenChange={(open) => {
          if (!open) setSpeedDialog(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          {speedDialog ? <SpeedTestPanel result={speedDialog} /> : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}

function speedStatusLine(s: SpeedTestResult): string {
  const bits = [`「${s.name}」${s.title}`];
  if (s.ttftMs != null) bits.push(`TTFT ${s.ttftMs}ms`);
  if (s.totalMs != null) bits.push(`总 ${s.totalMs}ms`);
  if (s.is403) bits.push("HTTP 403");
  else if (s.statusCode != null) bits.push(`HTTP ${s.statusCode}`);
  if (s.isCfBlock) bits.push("CF 拦截");
  return bits.join(" · ");
}

function formatMs(ms?: number | null): string {
  if (ms == null) return "—";
  if (ms >= 1000) return `${(ms / 1000).toFixed(ms >= 10_000 ? 1 : 2)}s`;
  return `${ms}ms`;
}

function SpeedTestPanel({ result }: { result: SpeedTestResult }) {
  const tone =
    !result.ok
      ? "text-destructive"
      : (result.totalMs ?? 0) < 2500
        ? "text-emerald-600"
        : (result.totalMs ?? 0) < 6000
          ? "text-amber-600"
          : "text-orange-600";

  return (
    <div className="space-y-4">
      <DialogHeader>
        <DialogTitle className="flex items-center gap-2">
          <Gauge className="h-4 w-4" />
          测速 · {result.name}
        </DialogTitle>
        <DialogDescription>
          {result.model ?? "未知模型"}
          {result.backend ? ` · ${result.backend}` : ""}
          {result.streamed ? " · stream" : " · non-stream"}
        </DialogDescription>
      </DialogHeader>

      <div className="grid grid-cols-3 gap-2">
        <MetricCell label="TTFT" value={formatMs(result.ttftMs)} emphasize={tone} />
        <MetricCell label="总耗时" value={formatMs(result.totalMs)} emphasize={tone} />
        <MetricCell
          label="HTTP"
          value={
            result.is403
              ? "403"
              : result.statusCode != null
                ? String(result.statusCode)
                : "—"
          }
          emphasize={
            result.is403 || result.isCfBlock
              ? "text-destructive"
              : result.ok
                ? "text-emerald-600"
                : "text-destructive"
          }
        />
      </div>

      <div className="grid grid-cols-2 gap-2">
        <MetricCell label="models" value={formatMs(result.modelsMs)} />
        <MetricCell
          label="拦截"
          value={result.isCfBlock ? "CF 是" : result.is403 ? "403" : "否"}
          emphasize={
            result.isCfBlock || result.is403 ? "text-destructive" : "text-muted-foreground"
          }
        />
      </div>

      <div className="space-y-1.5 rounded-md border border-border bg-background px-3 py-2.5">
        <div className="flex items-center justify-between gap-2">
          <span className={cn("text-sm font-medium", result.ok ? "text-foreground" : "text-destructive")}>
            {result.title}
          </span>
          <span
            className={cn(
              "rounded-full border px-1.5 py-0.5 text-[10px]",
              result.ok
                ? "border-border text-muted-foreground"
                : "border-destructive/30 text-destructive",
            )}
          >
            {result.ok ? "OK" : result.category}
          </span>
        </div>
        <p className="text-[12px] leading-relaxed text-foreground">{result.detail}</p>
        <p className="text-[12px] leading-relaxed text-muted-foreground">→ {result.hint}</p>
        {result.preview ? (
          <p className="truncate font-mono text-[11px] text-muted-foreground" title={result.preview}>
            预览：{result.preview}
          </p>
        ) : null}
        {result.url ? (
          <p className="truncate font-mono text-[10px] text-muted-foreground/80" title={result.url}>
            {result.url}
          </p>
        ) : null}
      </div>
    </div>
  );
}

function MetricCell({
  label,
  value,
  emphasize,
}: {
  label: string;
  value: string;
  emphasize?: string;
}) {
  return (
    <div className="rounded-md border border-border bg-background px-2.5 py-2 text-center">
      <div className={cn("tabular-nums text-sm font-semibold text-foreground", emphasize)}>
        {value}
      </div>
      <div className="mt-0.5 text-[10px] text-muted-foreground">{label}</div>
    </div>
  );
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

function healthLabel(h?: HealthResult, testing?: boolean): string | null {
  if (testing) return "检查中";
  if (!h) return null;
  if (h.ok) return "健康";
  switch (h.category) {
    case "auth":
      return "鉴权失败";
    case "rate_limit":
      return "限流";
    case "network":
      return "网络";
    case "config":
      return "配置";
    case "not_found":
      return "路径";
    case "server":
      return "服务端";
    default:
      return "异常";
  }
}

function StatusBar({
  current,
  defaultModelId,
  health,
  usage,
  usageLoading,
  onRefreshUsage,
}: {
  current: TokenProfile | null;
  defaultModelId: string | null;
  health?: HealthResult;
  usage: UsageSummary | null;
  usageLoading: boolean;
  onRefreshUsage: () => void;
}) {
  const host = current?.baseUrl
    ? current.baseUrl.replace(/^https?:\/\//, "").replace(/\/.*$/, "")
    : null;

  const currentTitle = current
    ? [
        current.name,
        current.modelId,
        current.baseUrl,
        defaultModelId ? `default · ${defaultModelId}` : null,
      ]
        .filter(Boolean)
        .join("\n")
    : "未启用供应商";

  return (
    <div className="flex items-center gap-2 border-b border-border px-5 py-2">
      <div className="min-w-0 flex-1 truncate text-xs" title={currentTitle}>
        {current ? (
          <>
            <span className="font-medium text-foreground">{current.name}</span>
            <span className="ml-1.5 text-muted-foreground">{current.modelId}</span>
            {host ? (
              <span className="ml-1.5 hidden text-muted-foreground/70 sm:inline">{host}</span>
            ) : null}
          </>
        ) : (
          <span className="text-muted-foreground">未启用供应商</span>
        )}
      </div>

      <div className="flex shrink-0 items-center gap-1.5">
        {health ? <HealthHoverChip health={health} /> : null}
        <UsageHoverChip
          usage={usage}
          loading={usageLoading}
          onRefresh={onRefreshUsage}
        />
      </div>
    </div>
  );
}

/**
 * Click-to-pin popover. Hover panels disappear when the cursor crosses the gap;
 * copy / scroll need a sticky panel.
 */
function ClickPopover({
  open,
  onOpenChange,
  trigger,
  children,
  align = "end",
  widthClass = "w-80",
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  trigger: ReactNode;
  children: ReactNode;
  align?: "start" | "end";
  widthClass?: string;
}) {
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (e: PointerEvent) => {
      const el = rootRef.current;
      if (!el) return;
      const t = e.target as Node | null;
      if (t && el.contains(t)) return;
      // Portaled dialogs (error log) live outside this tree — keep popover logic
      // from fighting them: only close if the click is not inside a dialog.
      if (t instanceof Element && t.closest('[role="dialog"]')) return;
      onOpenChange(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onOpenChange(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onOpenChange]);

  return (
    <div className="relative" ref={rootRef}>
      {trigger}
      {open ? (
        <div
          className={cn(
            "absolute top-full z-50 mt-1.5",
            widthClass,
            align === "end" ? "right-0" : "left-0",
          )}
          role="dialog"
          aria-modal="false"
        >
          <div className="max-h-[min(70vh,520px)] overflow-y-auto overflow-x-hidden rounded-lg border border-border bg-card p-3 shadow-lg shadow-black/20">
            {children}
          </div>
        </div>
      ) : null}
    </div>
  );
}

/** Light hover panel for non-interactive previews (health chip). */
function HoverPanel({
  children,
  panel,
  align = "end",
}: {
  children: ReactNode;
  panel: ReactNode;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const closeTimer = useRef<number | null>(null);

  const clearClose = () => {
    if (closeTimer.current != null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  };

  const scheduleClose = () => {
    clearClose();
    closeTimer.current = window.setTimeout(() => setOpen(false), 200);
  };

  useEffect(() => () => clearClose(), []);

  return (
    <div
      className="relative"
      onMouseEnter={() => {
        clearClose();
        setOpen(true);
      }}
      onMouseLeave={scheduleClose}
      onFocusCapture={() => {
        clearClose();
        setOpen(true);
      }}
      onBlurCapture={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) {
          scheduleClose();
        }
      }}
    >
      {children}
      {open ? (
        <div
          className={cn(
            // pt-1.5 acts as an invisible hover bridge (no dead gap).
            "absolute top-full z-50 w-80 pt-1.5",
            align === "end" ? "right-0" : "left-0",
          )}
          onMouseEnter={clearClose}
        >
          <div className="overflow-hidden rounded-lg border border-border bg-card p-3 shadow-lg shadow-black/20">
            {panel}
          </div>
        </div>
      ) : null}
    </div>
  );
}

function HealthHoverChip({ health }: { health: HealthResult }) {
  const latency = health.latencyMs ?? 0;
  // Map latency into a soft 0–100 bar for visualization (cap at 3s).
  const latencyPct = Math.min(100, Math.round((latency / 3000) * 100));
  const latencyTone =
    !health.ok
      ? "bg-destructive"
      : latency < 800
        ? "bg-emerald-500"
        : latency < 1500
          ? "bg-amber-500"
          : "bg-orange-500";

  return (
    <HoverPanel
      panel={
        <div className="space-y-3">
          <div className="flex items-start justify-between gap-2">
            <div>
              <div className="text-xs font-medium text-foreground">{health.title}</div>
              <div className="mt-0.5 text-[11px] text-muted-foreground">{health.name}</div>
            </div>
            <span
              className={cn(
                "inline-flex items-center gap-1 rounded-full border px-1.5 py-0.5 text-[10px]",
                health.ok
                  ? "border-border text-muted-foreground"
                  : "border-destructive/30 text-destructive",
              )}
            >
              <span
                className={cn(
                  "h-1.5 w-1.5 rounded-full",
                  health.ok ? "bg-emerald-500" : "bg-destructive",
                )}
              />
              {health.ok ? "OK" : health.category}
            </span>
          </div>

          {health.latencyMs != null ? (
            <div className="space-y-1.5">
              <div className="flex items-center justify-between text-[11px] text-muted-foreground">
                <span>延迟</span>
                <span className="tabular-nums text-foreground">{health.latencyMs}ms</span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className={cn("h-full rounded-full transition-all", latencyTone)}
                  style={{ width: `${Math.max(4, latencyPct)}%` }}
                />
              </div>
              <div className="flex justify-between text-[10px] text-muted-foreground/70">
                <span>快</span>
                <span>3s</span>
              </div>
            </div>
          ) : null}

          <div className="space-y-1 rounded-md border border-border bg-background px-2.5 py-2">
            <p className="text-[11px] leading-relaxed text-foreground">{health.detail}</p>
            {!health.ok ? (
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                → {health.hint}
              </p>
            ) : null}
          </div>

          {health.url ? (
            <p className="truncate font-mono text-[10px] text-muted-foreground" title={health.url}>
              {health.url}
            </p>
          ) : null}
        </div>
      }
    >
      <span
        className={cn(
          "inline-flex h-6 cursor-default items-center gap-1.5 rounded-full border px-2 text-[11px]",
          health.ok
            ? "border-border bg-background text-muted-foreground"
            : "border-destructive/30 bg-destructive/5 text-destructive",
        )}
      >
        <span
          className={cn(
            "h-1.5 w-1.5 shrink-0 rounded-full",
            health.ok ? "bg-emerald-500" : "bg-destructive",
          )}
        />
        {health.ok
          ? health.latencyMs != null
            ? `${health.latencyMs}ms`
            : "正常"
          : healthLabel(health) ?? health.title}
      </span>
    </HoverPanel>
  );
}

function formatLatency(ms?: number | null): string {
  if (ms == null || !Number.isFinite(ms)) return "—";
  if (ms >= 10_000) return `${(ms / 1000).toFixed(0)}s`;
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.round(ms)}ms`;
}

function issueKindLabel(kind: string): string {
  switch (kind) {
    case "rate_limit":
      return "限流";
    case "cancelled":
      return "取消";
    case "api_error":
      return "失败";
    default:
      return "问题";
  }
}

function formatErrorTime(at: number): string {
  if (!at) return "—";
  try {
    const d = new Date(at);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  } catch {
    return "—";
  }
}

function errorCopyText(e: UsageError): string {
  const lines = [
    `[${formatErrorTime(e.at)}] ${e.title}`,
    `kind: ${e.kind}`,
    e.model ? `model: ${e.model}` : null,
    e.sid ? `sid: ${e.sid}` : null,
    e.logMsg ? `log: ${e.logMsg}` : null,
    "",
    e.detail || e.message,
  ].filter((x) => x != null);
  return lines.join("\n");
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    try {
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.position = "fixed";
      ta.style.left = "-9999px";
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand("copy");
      document.body.removeChild(ta);
      return ok;
    } catch {
      return false;
    }
  }
}

function CopyErrorButton({
  error,
  className,
}: {
  error: UsageError;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className={cn(
        "inline-flex h-6 items-center gap-1 rounded-md border border-border px-1.5 text-[10px] text-muted-foreground transition-colors hover:text-foreground",
        className,
      )}
      title="复制完整错误"
      onClick={(ev) => {
        ev.stopPropagation();
        void copyText(errorCopyText(error)).then((ok) => {
          if (!ok) return;
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1200);
        });
      }}
    >
      {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
      {copied ? "已复制" : "复制"}
    </button>
  );
}

function ErrorLogDialog({
  open,
  onOpenChange,
  errors,
  windowHours,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  errors: UsageError[];
  windowHours: number;
}) {
  const [filter, setFilter] = useState<"actionable" | "all">("actionable");
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [copiedAll, setCopiedAll] = useState(false);

  const list = useMemo(() => {
    if (filter === "all") return errors;
    return errors.filter((e) => e.kind !== "cancelled");
  }, [errors, filter]);

  const copyAll = async () => {
    if (!list.length) return;
    const body = list.map((e, i) => `--- #${i + 1} ---\n${errorCopyText(e)}`).join("\n\n");
    const ok = await copyText(body);
    if (ok) {
      setCopiedAll(true);
      window.setTimeout(() => setCopiedAll(false), 1500);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(80vh,640px)] flex-col gap-0 overflow-hidden p-0 sm:max-w-lg">
        <DialogHeader className="shrink-0 space-y-1 border-b border-border px-4 py-3">
          <DialogTitle className="text-sm">错误记录</DialogTitle>
          <DialogDescription className="text-[11px]">
            近 {windowHours} 小时 · 每条独立事件 · 可复制全文
          </DialogDescription>
        </DialogHeader>

        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border px-4 py-2">
          <div className="flex items-center rounded-md border border-border p-0.5 text-[11px]">
            <button
              type="button"
              className={cn(
                "rounded px-2 py-0.5 transition-colors",
                filter === "actionable"
                  ? "bg-foreground text-background"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setFilter("actionable")}
            >
              失败 / 限流
            </button>
            <button
              type="button"
              className={cn(
                "rounded px-2 py-0.5 transition-colors",
                filter === "all"
                  ? "bg-foreground text-background"
                  : "text-muted-foreground hover:text-foreground",
              )}
              onClick={() => setFilter("all")}
            >
              全部
            </button>
          </div>
          <button
            type="button"
            disabled={!list.length}
            onClick={() => void copyAll()}
            className="inline-flex h-7 items-center gap-1 rounded-md border border-border px-2 text-[11px] text-muted-foreground hover:text-foreground disabled:opacity-40"
          >
            {copiedAll ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
            {copiedAll ? "已复制全部" : `复制全部 (${list.length})`}
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
          {list.length === 0 ? (
            <p className="px-1 py-8 text-center text-xs text-muted-foreground">
              {filter === "actionable" ? "暂无失败 / 限流记录" : "暂无错误记录"}
            </p>
          ) : (
            <ul className="space-y-2">
              {list.map((e) => {
                const isExpanded = expandedId === e.id;
                return (
                  <li
                    key={e.id}
                    className="rounded-lg border border-border bg-card"
                  >
                    <div className="flex items-start gap-2 px-2.5 py-2">
                      <button
                        type="button"
                        className="min-w-0 flex-1 text-left"
                        onClick={() =>
                          setExpandedId((id) => (id === e.id ? null : e.id))
                        }
                      >
                        <div className="flex flex-wrap items-center gap-1.5">
                          <span
                            className={cn(
                              "rounded px-1 py-px text-[9px]",
                              e.kind === "cancelled"
                                ? "bg-muted text-muted-foreground"
                                : "bg-destructive/10 text-destructive",
                            )}
                          >
                            {issueKindLabel(e.kind)}
                          </span>
                          <span className="text-[10px] tabular-nums text-muted-foreground">
                            {formatErrorTime(e.at)}
                          </span>
                          {e.model ? (
                            <span className="truncate font-mono text-[10px] text-muted-foreground">
                              {e.model}
                            </span>
                          ) : null}
                        </div>
                        <p className="mt-1 text-[12px] font-medium leading-snug text-foreground">
                          {e.title}
                        </p>
                        {!isExpanded ? (
                          <p className="mt-0.5 line-clamp-2 font-mono text-[10px] leading-relaxed text-muted-foreground">
                            {e.message}
                          </p>
                        ) : null}
                      </button>
                      <CopyErrorButton error={e} />
                    </div>
                    {isExpanded ? (
                      <div className="border-t border-border px-2.5 py-2">
                        <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words rounded-md bg-background p-2 font-mono text-[10px] leading-relaxed text-foreground">
                          {e.detail || e.message}
                        </pre>
                        <div className="mt-1.5 flex flex-wrap gap-x-3 gap-y-0.5 text-[10px] text-muted-foreground">
                          {e.logMsg ? <span>log: {e.logMsg}</span> : null}
                          {e.sid ? (
                            <span className="font-mono" title={e.sid}>
                              sid: {e.sid.slice(0, 8)}…
                            </span>
                          ) : null}
                        </div>
                      </div>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function UsageHoverChip({
  usage,
  loading,
  onRefresh,
}: {
  usage: UsageSummary | null;
  loading: boolean;
  onRefresh: () => void;
}) {
  const [panelOpen, setPanelOpen] = useState(false);
  const [errorLogOpen, setErrorLogOpen] = useState(false);
  const modelMax = Math.max(1, ...(usage?.byModel.map((m) => m.calls) ?? [1]));
  const cacheHitPct =
    usage && usage.promptTokens > 0
      ? Math.round((usage.cachedPromptTokens / usage.promptTokens) * 100)
      : null;

  const tokenRows = usage?.hasData
    ? [
        {
          key: "fresh",
          label: "新输入",
          value: usage.freshPromptTokens,
          tone: "bg-foreground/80",
        },
        {
          key: "cached",
          label: "缓存命中",
          value: usage.cachedPromptTokens,
          tone: "bg-emerald-500/70",
        },
        {
          key: "completion",
          label: "输出",
          value: usage.completionTokens,
          tone: "bg-foreground/45",
        },
        {
          key: "reasoning",
          label: "推理",
          value: usage.reasoningTokens,
          tone: "bg-foreground/25",
        },
      ].filter((r) => r.value > 0)
    : [];
  const tokenRowMax = Math.max(1, ...tokenRows.map((r) => r.value));

  const previewErrors = useMemo(() => {
    if (!usage?.recentErrors?.length) return [];
    const actionable = usage.recentErrors.filter((e) => e.kind !== "cancelled");
    return (actionable.length ? actionable : usage.recentErrors).slice(0, 5);
  }, [usage]);

  const chipAlert =
    usage && usage.rateLimitCount > 0
      ? { text: `${usage.rateLimitCount} 限流`, danger: true }
      : usage && usage.errorCount > 0
        ? { text: `${usage.errorCount} 失败`, danger: true }
        : null;

  return (
    <>
      <ClickPopover
        open={panelOpen}
        onOpenChange={setPanelOpen}
        align="end"
        widthClass="w-80"
        trigger={
          <button
            type="button"
            aria-expanded={panelOpen}
            aria-haspopup="dialog"
            title={panelOpen ? "点击关闭用量面板" : "点击打开用量面板（可复制错误）"}
            onClick={() => {
              setPanelOpen((v) => !v);
              // Soft refresh when opening so the list is current.
              if (!panelOpen && !loading) onRefresh();
            }}
            className={cn(
              "inline-flex h-6 max-w-[240px] items-center gap-1.5 rounded-full border bg-background px-2 text-[11px] transition-colors",
              panelOpen
                ? "border-foreground/40 text-foreground"
                : "border-border text-muted-foreground hover:text-foreground",
            )}
          >
            {loading ? (
              <Loader2 className="h-3 w-3 shrink-0 animate-spin" />
            ) : (
              <RefreshCw className="h-3 w-3 shrink-0 opacity-60" />
            )}
            {usage?.hasData ? (
              <>
                <span className="tabular-nums">{usage.totalCalls}</span>
                <span className="text-muted-foreground/40">·</span>
                <span className="tabular-nums">{formatTokens(usage.totalTokens)}</span>
                {chipAlert ? (
                  <>
                    <span className="text-muted-foreground/40">·</span>
                    <span className={chipAlert.danger ? "text-destructive" : undefined}>
                      {chipAlert.text}
                    </span>
                  </>
                ) : null}
              </>
            ) : (
              <span>用量</span>
            )}
          </button>
        }
      >
        {usage?.hasData ? (
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-2">
              <div>
                <div className="text-xs font-medium text-foreground">用量概览</div>
                <div className="text-[11px] text-muted-foreground">
                  近 {usage.windowHours} 小时 · 点击固定 · 可复制错误
                </div>
              </div>
              <button
                type="button"
                onClick={onRefresh}
                disabled={loading}
                className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-border text-muted-foreground hover:text-foreground disabled:opacity-50"
                title="刷新"
              >
                {loading ? (
                  <Loader2 className="h-3 w-3 animate-spin" />
                ) : (
                  <RefreshCw className="h-3 w-3" />
                )}
              </button>
            </div>

            <div className="grid grid-cols-4 gap-1.5">
              <StatCell label="成功" value={String(usage.totalCalls)} />
              <StatCell label="Tokens" value={formatTokens(usage.totalTokens)} />
              <StatCell
                label="TTFT"
                value={formatLatency(usage.avgTtftMs)}
                hint="首字延迟均值"
              />
              <StatCell
                label="总耗时"
                value={formatLatency(usage.avgLatencyMs)}
                hint="模型推理均值"
              />
            </div>

            <div className="space-y-1.5">
              <div className="flex items-center justify-between text-[11px] text-muted-foreground">
                <span>Token 构成</span>
                {cacheHitPct != null ? (
                  <span className="tabular-nums text-emerald-600 dark:text-emerald-400">
                    缓存命中 {cacheHitPct}%
                  </span>
                ) : (
                  <span className="tabular-nums text-foreground">
                    {formatTokens(usage.totalTokens)}
                  </span>
                )}
              </div>
              <ul className="space-y-1">
                {tokenRows.map((r) => {
                  const pct = Math.max(3, Math.round((r.value / tokenRowMax) * 100));
                  return (
                    <li key={r.key} className="flex items-center gap-2 text-[11px]">
                      <span className="w-14 shrink-0 text-muted-foreground">{r.label}</span>
                      <div className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full bg-muted">
                        <div
                          className={cn("h-full rounded-full", r.tone)}
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                      <span className="w-12 shrink-0 text-right tabular-nums text-foreground">
                        {formatTokens(r.value)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            </div>

            {usage.byModel.length > 0 ? (
              <div className="space-y-1.5">
                <div className="text-[11px] text-muted-foreground">按模型</div>
                <ul className="space-y-1.5">
                  {usage.byModel.slice(0, 5).map((m) => {
                    const pct = Math.max(4, Math.round((m.calls / modelMax) * 100));
                    const tokens =
                      m.promptTokens + m.completionTokens + m.reasoningTokens;
                    return (
                      <li key={m.model} className="space-y-0.5">
                        <div className="flex items-center justify-between gap-2 text-[11px]">
                          <span className="min-w-0 truncate font-mono text-foreground">
                            {m.model}
                          </span>
                          <span className="shrink-0 tabular-nums text-muted-foreground">
                            {m.calls} 次 · {formatTokens(tokens)}
                          </span>
                        </div>
                        <div className="h-1 overflow-hidden rounded-full bg-muted">
                          <div
                            className="h-full rounded-full bg-foreground/50"
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                      </li>
                    );
                  })}
                </ul>
              </div>
            ) : null}

            {(usage.rateLimitCount > 0 ||
              usage.errorCount > 0 ||
              usage.cancelledCount > 0) && (
              <div className="flex flex-wrap items-center gap-1.5">
                {usage.rateLimitCount > 0 ? (
                  <span className="rounded-md border border-destructive/30 bg-destructive/5 px-1.5 py-0.5 text-[10px] text-destructive">
                    限流 {usage.rateLimitCount}
                  </span>
                ) : null}
                {usage.errorCount > 0 ? (
                  <span className="rounded-md border border-destructive/30 bg-destructive/5 px-1.5 py-0.5 text-[10px] text-destructive">
                    失败 {usage.errorCount}
                  </span>
                ) : null}
                {usage.cancelledCount > 0 ? (
                  <span className="rounded-md border border-border px-1.5 py-0.5 text-[10px] text-muted-foreground">
                    取消 {usage.cancelledCount}
                  </span>
                ) : null}
              </div>
            )}

            {usage.recentErrors.length > 0 ? (
              <div className="space-y-1.5 border-t border-border pt-2">
                <div className="flex items-center justify-between gap-2">
                  <div className="text-[11px] text-muted-foreground">最近错误</div>
                  <button
                    type="button"
                    className="text-[11px] text-foreground underline-offset-2 hover:underline"
                    onClick={() => setErrorLogOpen(true)}
                  >
                    查看全部 ({usage.recentErrors.length})
                  </button>
                </div>
                <ul className="space-y-1.5">
                  {previewErrors.map((e) => (
                    <li
                      key={e.id}
                      className="rounded-md border border-border bg-background px-2 py-1.5"
                    >
                      <div className="flex items-start gap-1.5">
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-1">
                            <span
                              className={cn(
                                "rounded px-1 py-px text-[9px]",
                                e.kind === "cancelled"
                                  ? "bg-muted text-muted-foreground"
                                  : "bg-destructive/10 text-destructive",
                              )}
                            >
                              {issueKindLabel(e.kind)}
                            </span>
                            <span className="text-[10px] tabular-nums text-muted-foreground">
                              {formatErrorTime(e.at)}
                            </span>
                          </div>
                          <p className="mt-0.5 text-[11px] font-medium leading-snug text-foreground">
                            {e.title}
                          </p>
                          <p
                            className="mt-0.5 line-clamp-2 font-mono text-[10px] leading-relaxed text-muted-foreground"
                            title={e.message}
                          >
                            {e.message}
                          </p>
                        </div>
                        <CopyErrorButton error={e} />
                      </div>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
          </div>
        ) : (
          <div className="space-y-2 py-1 text-center">
            <p className="text-xs text-muted-foreground">暂无近 24h 用量日志</p>
            <button
              type="button"
              onClick={onRefresh}
              disabled={loading}
              className="text-[11px] text-foreground underline-offset-2 hover:underline"
            >
              {loading ? "读取中…" : "刷新试试"}
            </button>
          </div>
        )}
      </ClickPopover>

      <ErrorLogDialog
        open={errorLogOpen}
        onOpenChange={setErrorLogOpen}
        errors={usage?.recentErrors ?? []}
        windowHours={usage?.windowHours ?? 24}
      />
    </>
  );
}

function StatCell({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div
      className="rounded-md border border-border bg-background px-2 py-1.5 text-center"
      title={hint}
    >
      <div className="tabular-nums text-xs font-medium text-foreground">{value}</div>
      <div className="text-[10px] text-muted-foreground">{label}</div>
    </div>
  );
}

function ProviderCard({
  provider,
  isCurrent,
  busy,
  enabling,
  speedTesting,
  health,
  speed,
  onEnable,
  onSpeedTest,
  onShowSpeed,
  onEdit,
  onDelete,
}: {
  provider: TokenProfile;
  isCurrent: boolean;
  busy: boolean;
  enabling: boolean;
  speedTesting: boolean;
  health?: HealthResult;
  speed?: SpeedTestResult;
  onEnable: () => void;
  onSpeedTest: () => void;
  onShowSpeed: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const initial = provider.name.slice(0, 1).toUpperCase() || "G";
  const host = provider.baseUrl
    ? provider.baseUrl.replace(/^https?:\/\//, "").replace(/\/.*$/, "")
    : null;

  const speedText = speedTesting
    ? "测速中"
    : speed
      ? speed.is403
        ? "403"
        : speed.ttftMs != null
          ? `TTFT ${formatMs(speed.ttftMs)}`
          : speed.totalMs != null
            ? `总 ${formatMs(speed.totalMs)}`
            : speed.title
      : null;
  const metaBits = [
    enabling ? "启用中" : isCurrent ? "使用中" : null,
    provider.tokenSaved ? "密钥" : "无密钥",
    speedText,
  ].filter(Boolean);

  const speedTitle = speed
    ? `${speed.title}\nmodels ${formatMs(speed.modelsMs)} · TTFT ${formatMs(speed.ttftMs)} · 总 ${formatMs(
        speed.totalMs,
      )}\nHTTP ${speed.statusCode ?? "—"}${speed.is403 ? " · 403" : ""}${
        speed.isCfBlock ? " · CF" : ""
      }`
    : "一键测速（含 /models + TTFT + 总耗时）";

  return (
    <div
      className={cn(
        "group flex aspect-square flex-col rounded-lg border bg-card p-3 transition-colors",
        isCurrent ? "border-foreground/40" : "border-border hover:border-foreground/25",
        (health && !health.ok) || (speed && !speed.ok) ? "border-destructive/40" : null,
      )}
    >
      <div className="flex items-start justify-between gap-1">
        <div
          className={cn(
            "grid h-8 w-8 place-items-center rounded-md text-xs font-semibold",
            isCurrent
              ? "bg-foreground text-background"
              : "border border-border text-muted-foreground",
          )}
        >
          {initial}
        </div>

        <div className="flex items-center">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            title={speedTitle}
            disabled={busy || speedTesting}
            onClick={onSpeedTest}
          >
            {speedTesting ? <Loader2 className="animate-spin" /> : <Gauge />}
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7">
                <MoreHorizontal />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={onEdit}>
                <Pencil className="mr-2 h-3.5 w-3.5" />
                编辑
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                disabled={isCurrent}
                onClick={onDelete}
              >
                <Trash2 className="mr-2 h-3.5 w-3.5" />
                删除
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div className="mt-3 min-h-0 flex-1 space-y-1">
        {metaBits.length > 0 ? (
          <p
            className={cn(
              "truncate text-[11px]",
              speed && !speed.ok ? "text-destructive" : "text-muted-foreground",
            )}
            title={speedTitle}
          >
            {metaBits.join(" · ")}
          </p>
        ) : null}
        <h3 className="line-clamp-2 text-sm font-medium leading-snug">{provider.name}</h3>
        <p className="truncate font-mono text-[11px] text-muted-foreground" title={provider.modelId}>
          {provider.modelId}
        </p>
        {host ? (
          <p className="truncate text-[11px] text-muted-foreground" title={provider.baseUrl ?? undefined}>
            {host}
          </p>
        ) : null}
        {speed && !speed.ok ? (
          <button
            type="button"
            onClick={onShowSpeed}
            className="line-clamp-2 text-left text-[11px] text-destructive/90 hover:underline"
            title={speed.hint}
          >
            {speed.is403 ? "HTTP 403 · " : ""}
            {speed.hint}
          </button>
        ) : speed?.ok ? (
          <button
            type="button"
            onClick={onShowSpeed}
            className="line-clamp-1 text-left text-[11px] text-muted-foreground hover:text-foreground"
            title={speedTitle}
          >
            TTFT {formatMs(speed.ttftMs)} · 总 {formatMs(speed.totalMs)}
          </button>
        ) : null}
      </div>

      <div className="mt-2 flex gap-1.5">
        <Button size="sm" variant="secondary" className="h-7 flex-1" onClick={onEdit}>
          编辑
        </Button>
        {isCurrent && !enabling ? (
          <Button size="sm" variant="secondary" disabled className="h-7 flex-1">
            <Check />
            当前
          </Button>
        ) : (
          <Button
            size="sm"
            disabled={busy || enabling}
            className="h-7 flex-1"
            onClick={onEnable}
          >
            {enabling ? <Loader2 className="animate-spin" /> : null}
            {enabling ? "启用中" : "启用"}
          </Button>
        )}
      </div>
    </div>
  );
}

function EmptyState({
  onCreate,
  onTools,
}: {
  onCreate: () => void;
  onTools: () => void;
}) {
  return (
    <div className="grid place-items-center rounded-lg border border-dashed border-border px-6 py-20 text-center">
      <h2 className="text-sm font-medium">还没有供应商</h2>
      <p className="mt-1 max-w-xs text-xs text-muted-foreground">
        导入现有 config，或添加新供应商。也可以先去「工具箱」体检环境。
      </p>
      <div className="mt-4 flex gap-2">
        <Button size="sm" variant="secondary" onClick={onTools}>
          工具箱
        </Button>
        <Button size="sm" onClick={onCreate}>
          <Plus />
          添加
        </Button>
      </div>
    </div>
  );
}
