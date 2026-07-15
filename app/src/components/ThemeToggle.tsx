import { useEffect, useState } from "react";
import { Monitor, Moon, Sun } from "lucide-react";
import {
  THEME_LABEL,
  applyTheme,
  cycleTheme,
  getStoredTheme,
  type ThemeMode,
} from "@/lib/theme";
import { Button } from "@/components/ui/button";

function ThemeIcon({ mode }: { mode: ThemeMode }) {
  if (mode === "light") return <Sun />;
  if (mode === "dark") return <Moon />;
  return <Monitor />;
}

export function ThemeToggle({ className }: { className?: string }) {
  const [mode, setMode] = useState<ThemeMode>(() => getStoredTheme());

  useEffect(() => {
    applyTheme(mode);
  }, [mode]);

  // When following system, re-apply if OS theme flips while the app is open.
  useEffect(() => {
    if (mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyTheme("system");
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [mode]);

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={className ?? "h-8 w-8 shrink-0"}
      title={`主题：${THEME_LABEL[mode]}（点击切换）`}
      aria-label={`主题：${THEME_LABEL[mode]}`}
      onClick={() => setMode((m) => cycleTheme(m))}
    >
      <ThemeIcon mode={mode} />
    </Button>
  );
}
