import { useEffect, useState } from "react";
import { ArrowLeft, Eye, EyeOff, Save, Zap } from "lucide-react";
import type { TokenProfile } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";

const API_BACKENDS = [
  { value: "responses", label: "responses" },
  { value: "chat_completions", label: "chat_completions" },
  { value: "messages", label: "messages" },
] as const;

export type EditorForm = {
  name: string;
  modelId: string;
  apiModel: string;
  modelAlias: string;
  description: string;
  baseUrl: string;
  envKey: string;
  apiBackend: string;
  contextWindow: string;
  maxCompletionTokens: string;
  configPath: string;
  setAsDefault: boolean;
  token: string;
};

export function profileToForm(profile: TokenProfile, token = ""): EditorForm {
  return {
    name: profile.name,
    modelId: profile.modelId,
    apiModel: profile.apiModel ?? "",
    modelAlias: profile.modelAlias ?? "",
    description: profile.description ?? "",
    baseUrl: profile.baseUrl ?? "",
    envKey: profile.envKey ?? "",
    apiBackend: profile.apiBackend || "responses",
    contextWindow: profile.contextWindow?.toString() ?? "",
    maxCompletionTokens: profile.maxCompletionTokens?.toString() ?? "",
    configPath: profile.configPath,
    setAsDefault: profile.setAsDefault !== false,
    token,
  };
}

/** Blank create form — xAI-style examples live only in input placeholders. */
export function emptyForm(): EditorForm {
  return {
    name: "",
    modelId: "",
    apiModel: "",
    modelAlias: "",
    description: "",
    baseUrl: "",
    envKey: "",
    apiBackend: "responses",
    contextWindow: "",
    maxCompletionTokens: "",
    configPath: "",
    setAsDefault: true,
    token: "",
  };
}

type Props = {
  mode: "create" | "edit";
  title: string;
  initial: EditorForm;
  busy?: boolean;
  isCurrent?: boolean;
  onBack: () => void;
  onSave: (form: EditorForm) => void | Promise<void>;
  onEnable?: (form: EditorForm) => void | Promise<void>;
};

export function ProviderEditor({
  mode,
  title,
  initial,
  busy,
  isCurrent,
  onBack,
  onSave,
  onEnable,
}: Props) {
  const [form, setForm] = useState<EditorForm>(initial);
  const [reveal, setReveal] = useState(false);

  useEffect(() => {
    setForm(initial);
    setReveal(false);
  }, [initial]);

  const patch = (partial: Partial<EditorForm>) =>
    setForm((prev) => ({ ...prev, ...partial }));

  const canSave =
    form.name.trim().length > 0 &&
    form.modelId.trim().length > 0 &&
    (mode === "edit" || form.token.trim().length > 0);

  return (
    <div className="w-full">
      <div className="sticky top-0 z-20 border-b border-border bg-background/95 backdrop-blur-sm">
        <div className="mx-auto flex max-w-3xl items-center justify-between gap-3 px-5 py-2.5">
          <div className="flex min-w-0 items-center gap-1.5">
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onBack}>
              <ArrowLeft />
            </Button>
            <div className="min-w-0">
              <h1 className="truncate text-sm font-medium tracking-tight">{title}</h1>
              <p className="text-[11px] text-muted-foreground">
                [model.&lt;id&gt;] 字段
              </p>
            </div>
          </div>
          <div className="flex shrink-0 gap-1.5">
            {mode === "edit" && onEnable && !isCurrent ? (
              <Button
                variant="secondary"
                size="sm"
                disabled={busy || !canSave}
                onClick={() => void onEnable(form)}
              >
                <Zap />
                保存并启用
              </Button>
            ) : null}
            <Button size="sm" disabled={busy || !canSave} onClick={() => void onSave(form)}>
              <Save />
              {mode === "create" ? "添加备用" : "保存"}
            </Button>
          </div>
        </div>
      </div>

      <div className="mx-auto flex w-full max-w-3xl flex-col gap-3 px-5 py-5">
      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h2 className="text-xs font-medium text-muted-foreground">供应商</h2>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <Field label="本地名称" hint="App 卡片上显示">
            <Input
              value={form.name}
              onChange={(e) => patch({ name: e.target.value })}
              placeholder="xAI Grok"
            />
          </Field>
          <Field
            label="模型 ID（section）"
            hint="写入 config 的 [model.xai-grok] 段名"
          >
            <Input
              value={form.modelId}
              onChange={(e) => patch({ modelId: e.target.value })}
              placeholder="xai-grok"
            />
          </Field>
        </div>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h2 className="text-xs font-medium text-muted-foreground">模型段</h2>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <Field label="model" hint="发给 API 的模型名，如 grok-4.5">
            <Input
              value={form.apiModel}
              onChange={(e) => patch({ apiModel: e.target.value })}
              placeholder="grok-4.5"
            />
          </Field>
          <Field label="name" hint="Grok 模型选择器里的显示名">
            <Input
              value={form.modelAlias}
              onChange={(e) => patch({ modelAlias: e.target.value })}
              placeholder="Grok 4.5"
            />
          </Field>
          <div className="md:col-span-2">
            <Field label="description">
              <Input
                value={form.description}
                onChange={(e) => patch({ description: e.target.value })}
                placeholder="Official xAI API"
              />
            </Field>
          </div>
          <div className="md:col-span-2">
            <Field label="base_url" hint="官方或 OpenAI 兼容端点">
              <Input
                value={form.baseUrl}
                onChange={(e) => patch({ baseUrl: e.target.value })}
                placeholder="https://api.x.ai/v1"
              />
            </Field>
          </div>
          <Field
            label="api_key / Token"
            hint={
              isCurrent
                ? "保存会立即写入 config.toml"
                : "先保存在本地；点「保存并启用」时才写入 config.toml"
            }
          >
            <div className="relative">
              <Input
                type={reveal ? "text" : "password"}
                value={form.token}
                className="pr-12"
                autoComplete="off"
                spellCheck={false}
                placeholder={mode === "edit" ? "留空则保持本地已存的值" : "sk-..."}
                onChange={(e) => patch({ token: e.target.value })}
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="absolute right-1 top-1/2 h-7 w-7 -translate-y-1/2"
                onClick={() => setReveal((v) => !v)}
              >
                {reveal ? <EyeOff className="h-3.5 w-3.5" /> : <Eye className="h-3.5 w-3.5" />}
              </Button>
            </div>
          </Field>
          <Field label="env_key" hint="如 XAI_API_KEY；有 api_key 时优先用 api_key">
            <Input
              value={form.envKey}
              onChange={(e) => patch({ envKey: e.target.value })}
              placeholder="XAI_API_KEY"
            />
          </Field>
          <Field label="api_backend" hint="官方 xAI 推荐 responses">
            <Select
              value={form.apiBackend || "responses"}
              onValueChange={(v) => patch({ apiBackend: v })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {API_BACKENDS.map((b) => (
                  <SelectItem key={b.value} value={b.value}>
                    {b.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
          <Field label="context_window" hint="用于 auto-compact 阈值">
            <Input
              inputMode="numeric"
              value={form.contextWindow}
              onChange={(e) => patch({ contextWindow: e.target.value })}
              placeholder="200000"
            />
          </Field>
          <Field label="max_completion_tokens">
            <Input
              inputMode="numeric"
              value={form.maxCompletionTokens}
              onChange={(e) => patch({ maxCompletionTokens: e.target.value })}
              placeholder="8192"
            />
          </Field>
        </div>

        <label className="flex items-center gap-2.5 text-sm text-muted-foreground">
          <Checkbox
            checked={form.setAsDefault}
            onCheckedChange={(c) => patch({ setAsDefault: c === true })}
          />
          <span>
            启用时写入 <code className="font-mono text-[11px]">[models] default</code>
          </span>
        </label>
      </section>

      <section className="space-y-3 rounded-lg border border-border bg-card p-4">
        <h2 className="text-xs font-medium text-muted-foreground">其它</h2>
        <Field label="config 路径">
          <Input
            value={form.configPath}
            onChange={(e) => patch({ configPath: e.target.value })}
            placeholder="~/.grok/config.toml"
          />
        </Field>
      </section>
      </div>
    </div>
  );
}

export function ConfigRawEditor({
  path,
  value,
  busy,
  onChange,
  onReload,
  onSave,
}: {
  path?: string | null;
  value: string;
  busy?: boolean;
  onChange: (v: string) => void;
  onReload: () => void;
  onSave: () => void;
}) {
  return (
    <section className="mx-auto w-full max-w-3xl space-y-2 px-5 pb-8">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h2 className="text-xs font-medium text-muted-foreground">config.toml</h2>
          <p className="truncate text-[11px] text-muted-foreground">
            {path || "~/.grok/config.toml"}
          </p>
        </div>
        <div className="flex gap-1.5">
          <Button variant="ghost" size="sm" disabled={busy} onClick={onReload}>
            重新加载
          </Button>
          <Button variant="secondary" size="sm" disabled={busy} onClick={onSave}>
            <Save />
            保存
          </Button>
        </div>
      </div>
      <Textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false}
        className="min-h-[320px] font-mono text-xs leading-relaxed"
        placeholder="# ~/.grok/config.toml"
      />
      <p className="text-[11px] text-muted-foreground">
        保存前自动备份 · 文件中含明文 api_key
      </p>
    </section>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5">
      <Label>{label}</Label>
      {children}
      {hint ? <p className="text-[11px] leading-relaxed text-muted-foreground">{hint}</p> : null}
    </div>
  );
}
