import { useCallback, useEffect, useRef, useState } from "react";
import type { Update } from "@tauri-apps/plugin-updater";
import { ArrowUpCircle, Loader2, RefreshCw } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
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
  checkForUpdate,
  downloadAndInstallUpdate,
  formatUpdateError,
  relaunchApp,
  toUpdateInfo,
  type UpdateInfo,
  type UpdateProgress,
} from "@/lib/updater";
import { cn } from "@/lib/utils";

type Props = {
  onStatus?: (msg: string) => void;
  /** Auto-check shortly after mount (production only). */
  autoCheck?: boolean;
  className?: string;
};

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function UpdateChecker({
  onStatus,
  autoCheck = true,
  className,
}: Props) {
  const [appVersion, setAppVersion] = useState<string>("");
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const pendingRef = useRef<Update | null>(null);
  const autoDone = useRef(false);

  useEffect(() => {
    void getVersion()
      .then(setAppVersion)
      .catch(() => setAppVersion(""));
  }, []);

  const runCheck = useCallback(
    async (opts?: { silent?: boolean }) => {
      const silent = opts?.silent ?? false;
      setChecking(true);
      try {
        const update = await checkForUpdate();
        if (!update) {
          pendingRef.current = null;
          setInfo(null);
          if (!silent) {
            onStatus?.(
              appVersion
                ? `已是最新版本（v${appVersion}）`
                : "已是最新版本",
            );
          }
          return;
        }
        pendingRef.current = update;
        setInfo(toUpdateInfo(update));
        setDialogOpen(true);
        onStatus?.(`发现新版本 v${update.version}`);
      } catch (err) {
        if (!silent) {
          onStatus?.(formatUpdateError(err));
        }
      } finally {
        setChecking(false);
      }
    },
    [appVersion, onStatus],
  );

  useEffect(() => {
    if (!autoCheck || autoDone.current) return;
    autoDone.current = true;
    // Defer so first paint / tray warm-up aren't competing for network.
    const t = window.setTimeout(() => {
      void runCheck({ silent: true });
    }, 2500);
    return () => window.clearTimeout(t);
  }, [autoCheck, runCheck]);

  const handleInstall = async () => {
    const update = pendingRef.current;
    if (!update || installing) return;
    setInstalling(true);
    setProgress({ downloaded: 0, total: null });
    try {
      onStatus?.(`正在下载 v${update.version}…`);
      await downloadAndInstallUpdate(update, setProgress);
      onStatus?.("更新已安装，正在重启…");
      await relaunchApp();
    } catch (err) {
      onStatus?.(formatUpdateError(err));
      setInstalling(false);
    }
  };

  const pct =
    progress && progress.total && progress.total > 0
      ? Math.min(100, Math.round((progress.downloaded / progress.total) * 100))
      : null;

  return (
    <>
      <div className={cn("flex items-center gap-2", className)}>
        {appVersion ? (
          <span className="tabular-nums text-muted-foreground">v{appVersion}</span>
        ) : null}
        <button
          type="button"
          disabled={checking || installing}
          onClick={() => void runCheck({ silent: false })}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors hover:bg-muted hover:text-foreground disabled:opacity-50"
          title="检查更新"
        >
          {checking ? (
            <Loader2 className="h-3 w-3 animate-spin" />
          ) : (
            <RefreshCw className="h-3 w-3" />
          )}
          <span>检查更新</span>
        </button>
      </div>

      <Dialog
        open={dialogOpen}
        onOpenChange={(open) => {
          if (installing) return;
          setDialogOpen(open);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <ArrowUpCircle className="h-5 w-5" />
              发现新版本
            </DialogTitle>
            <DialogDescription>
              {info ? (
                <>
                  当前 v{info.currentVersion} → 最新{" "}
                  <span className="font-medium text-foreground">v{info.version}</span>
                </>
              ) : (
                "有可用更新"
              )}
            </DialogDescription>
          </DialogHeader>

          {info?.notes ? (
            <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded-md border border-border bg-muted/40 p-3 text-xs text-muted-foreground">
              {info.notes}
            </pre>
          ) : null}

          {installing ? (
            <div className="space-y-1.5 text-xs text-muted-foreground">
              <div className="flex justify-between">
                <span>下载中…</span>
                <span className="tabular-nums">
                  {progress
                    ? `${formatBytes(progress.downloaded)}${
                        progress.total != null
                          ? ` / ${formatBytes(progress.total)}`
                          : ""
                      }`
                    : ""}
                  {pct != null ? ` · ${pct}%` : ""}
                </span>
              </div>
              <div className="h-1.5 overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full bg-foreground transition-all"
                  style={{ width: `${pct ?? 15}%` }}
                />
              </div>
            </div>
          ) : null}

          <DialogFooter>
            <Button
              variant="secondary"
              disabled={installing}
              onClick={() => setDialogOpen(false)}
            >
              稍后
            </Button>
            <Button disabled={installing} onClick={() => void handleInstall()}>
              {installing ? (
                <>
                  <Loader2 className="animate-spin" />
                  安装中
                </>
              ) : (
                "下载并安装"
              )}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
