import { useEffect, useRef, useState } from "react";
import { motion, AnimatePresence } from "motion/react";
import type { PrismChatMessage, PrismUsage } from "../../../types/prism";
import { SYSTEM_COLORS } from "../ui/primitives";
import { ArrowUpIcon, BoltIcon, EyeIcon, SparklesIcon, TrashIcon } from "../ui/icons";

interface PrismModuleProps {
  messages: PrismChatMessage[];
  actionMode: boolean;
  usage?: PrismUsage;
  isLoading: boolean;
  error: string | null;
  onSendMessage: (message: string) => Promise<void>;
  onToggleActionMode: (enabled: boolean) => void;
  onClearChat: () => void;
  /** Whether Prism may read the foreground app (title + exe) for context. */
  activeAppContext: boolean;
  onToggleActiveAppContext: (enabled: boolean) => void;
  /** True when no Groq key is configured (env var or saved from this screen). */
  needsApiKey?: boolean;
  onSaveApiKey?: (key: string) => Promise<boolean>;
}

function ApiKeySetup({ onSave }: { onSave: (key: string) => Promise<boolean> }) {
  const [key, setKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [failed, setFailed] = useState(false);
  const submit = async () => {
    if (!key.trim() || saving) return;
    setSaving(true);
    setFailed(false);
    const ok = await onSave(key.trim());
    setSaving(false);
    if (!ok) setFailed(true);
  };
  return (
    <motion.div
      className="flex flex-col items-center text-center h-full justify-center px-2"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
    >
      <div
        className="w-11 h-11 rounded-[14px] flex items-center justify-center text-white mb-2"
        style={{ background: PRISM_GRADIENT }}
      >
        <SparklesIcon size={22} />
      </div>
      <span className="text-white text-[14px] font-semibold">Connect Prism</span>
      <span className="text-white/45 text-[11.5px] mb-3 leading-snug">
        Prism runs on Groq. Paste a free API key from console.groq.com — it's stored only on this PC.
      </span>
      <form
        className="w-full flex items-center rounded-[20px] bg-white/[0.08] focus-within:bg-white/[0.11] transition-colors"
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <input
          type="password"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="gsk_…"
          autoComplete="off"
          spellCheck={false}
          className="flex-1 min-w-0 bg-transparent pl-3.5 pr-1 h-[38px] text-[12.5px] text-white outline-none"
          aria-label="Groq API key"
        />
        <motion.button
          type="submit"
          className="m-[5px] h-[28px] px-3.5 rounded-full text-[11.5px] font-semibold disabled:opacity-30"
          style={{ background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" }}
          disabled={!key.trim() || saving}
          whileTap={{ scale: 0.92 }}
        >
          {saving ? "Saving…" : "Save"}
        </motion.button>
      </form>
      {failed && (
        <span className="text-[11px] mt-2" style={{ color: SYSTEM_COLORS.red }}>
          Couldn't save the key. Check the logs for details.
        </span>
      )}
    </motion.div>
  );
}

const SUGGESTIONS = [
  "Start a 25 minute focus timer",
  "What's playing right now?",
  "Summarize my notifications",
  "Set volume to 30%",
];

const PRISM_GRADIENT = `linear-gradient(135deg, ${SYSTEM_COLORS.purple}, ${SYSTEM_COLORS.blue} 55%, ${SYSTEM_COLORS.teal})`;

function formatTime(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false });
}

function ToggleChip({
  on,
  onClick,
  icon,
  label,
  title,
  tint,
}: {
  on: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  title: string;
  tint: string;
}) {
  return (
    <motion.button
      type="button"
      aria-pressed={on}
      title={title}
      className="flex items-center gap-1 h-[24px] px-2 rounded-full text-[10.5px] font-semibold transition-colors"
      style={
        on
          ? { background: `color-mix(in srgb, ${tint} 22%, transparent)`, color: tint }
          : { background: "rgba(255,255,255,0.07)", color: "rgba(255,255,255,0.45)" }
      }
      onClick={onClick}
      whileTap={{ scale: 0.92 }}
    >
      {icon}
      {label}
    </motion.button>
  );
}

export function PrismModule({
  messages,
  actionMode,
  usage: _usage,
  isLoading,
  error,
  onSendMessage,
  onToggleActionMode,
  onClearChat,
  activeAppContext,
  onToggleActiveAppContext,
  needsApiKey = false,
  onSaveApiKey,
}: PrismModuleProps) {
  const [inputValue, setInputValue] = useState("");
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!scrollRef.current) return;
    scrollRef.current.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [messages, isLoading]);

  const send = (text: string) => {
    const message = text.trim();
    if (!message || isLoading) return;
    setInputValue("");
    void onSendMessage(message);
  };

  const canSend = inputValue.trim().length > 0 && !isLoading;

  const keyMissing = needsApiKey || (error?.includes("PRISM_NO_API_KEY") ?? false);
  if (keyMissing && onSaveApiKey) {
    return <ApiKeySetup onSave={onSaveApiKey} />;
  }

  return (
    <div className="flex flex-col h-full min-h-0 w-full max-w-full">
      {/* Controls */}
      <div className="flex items-center gap-1.5 mb-2 flex-shrink-0">
        <ToggleChip
          on={actionMode}
          onClick={() => onToggleActionMode(!actionMode)}
          icon={<BoltIcon size={11} />}
          label={actionMode ? "Actions on" : "Chat only"}
          title="When on, Prism can directly control timer, media, and volume."
          tint={SYSTEM_COLORS.green}
        />
        <ToggleChip
          on={activeAppContext}
          onClick={() => onToggleActiveAppContext(!activeAppContext)}
          icon={<EyeIcon size={11} strokeWidth={2.4} />}
          label="Sees app"
          title="When on, Prism can see your active app's name and window title for context. No screenshots, nothing stored."
          tint={SYSTEM_COLORS.blue}
        />
        {messages.length > 0 && (
          <motion.button
            type="button"
            className="ml-auto w-[24px] h-[24px] rounded-full flex items-center justify-center bg-white/[0.07] text-white/45 hover:text-white transition-colors"
            onClick={onClearChat}
            aria-label="Clear chat"
            title="Clear chat"
            whileTap={{ scale: 0.9 }}
          >
            <TrashIcon size={12} />
          </motion.button>
        )}
      </div>

      {/* Conversation */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden flex flex-col gap-1.5 pb-2">
        {messages.length === 0 && !isLoading && (
          <motion.div
            className="flex flex-col items-center text-center pt-3"
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
          >
            <motion.div
              className="w-11 h-11 rounded-[14px] flex items-center justify-center text-white mb-2"
              style={{ background: PRISM_GRADIENT, boxShadow: `0 6px 24px color-mix(in srgb, ${SYSTEM_COLORS.purple} 45%, transparent)` }}
              animate={{ rotate: [0, 6, -6, 0] }}
              transition={{ duration: 6, repeat: Infinity, ease: "easeInOut" }}
            >
              <SparklesIcon size={22} />
            </motion.div>
            <span className="text-white text-[14px] font-semibold">Ask Prism</span>
            <span className="text-white/40 text-[11.5px] mb-3">Control your PC or ask about what's happening</span>
            <div className="flex flex-wrap justify-center gap-1.5">
              {SUGGESTIONS.map((s) => (
                <motion.button
                  key={s}
                  type="button"
                  className="h-7 px-3 rounded-full bg-white/[0.07] hover:bg-white/[0.12] text-white/70 hover:text-white text-[11px] font-medium transition-colors"
                  onClick={() => send(s)}
                  whileTap={{ scale: 0.94 }}
                >
                  {s}
                </motion.button>
              ))}
            </div>
          </motion.div>
        )}

        <AnimatePresence initial={false}>
          {messages.map((item) => {
            const isUser = item.role === "user";
            return (
              <motion.div
                key={item.id}
                className={`flex flex-col ${isUser ? "items-end" : "items-start"}`}
                initial={{ opacity: 0, y: 10, scale: 0.96 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                transition={{ type: "spring", stiffness: 500, damping: 34 }}
                style={{ originX: isUser ? 1 : 0 }}
              >
                <div
                  className={`max-w-[84%] px-3 py-2 text-[12.5px] leading-snug whitespace-pre-wrap break-words ${
                    isUser ? "rounded-[18px] rounded-br-[6px]" : "rounded-[18px] rounded-bl-[6px] bg-white/[0.1] text-white"
                  }`}
                  style={isUser ? { background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" } : undefined}
                  dir="auto"
                >
                  {item.content}
                </div>
                <span className="text-[9.5px] text-white/30 mt-0.5 px-1.5 tabular-nums">{formatTime(item.timestamp)}</span>
              </motion.div>
            );
          })}
        </AnimatePresence>

        {isLoading && (
          <motion.div
            className="self-start flex items-center gap-1 rounded-[18px] rounded-bl-[6px] bg-white/[0.1] px-3.5 h-[34px]"
            initial={{ opacity: 0, scale: 0.9 }}
            animate={{ opacity: 1, scale: 1 }}
            aria-label="Prism is thinking"
          >
            {[0, 1, 2].map((i) => (
              <motion.span
                key={i}
                className="w-[6px] h-[6px] rounded-full bg-white/60"
                animate={{ y: [0, -3, 0], opacity: [0.4, 1, 0.4] }}
                transition={{ duration: 0.9, repeat: Infinity, delay: i * 0.15 }}
              />
            ))}
          </motion.div>
        )}

        {error && (
          <div
            className="text-[11.5px] rounded-[12px] px-3 py-2"
            style={{ color: SYSTEM_COLORS.red, background: `color-mix(in srgb, ${SYSTEM_COLORS.red} 14%, transparent)` }}
          >
            {error}
          </div>
        )}
      </div>

      {/* Composer */}
      <form
        className="flex-shrink-0 relative flex items-end rounded-[20px] bg-white/[0.08] focus-within:bg-white/[0.11] transition-colors"
        onSubmit={(e) => {
          e.preventDefault();
          send(inputValue);
        }}
      >
        <textarea
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          placeholder="Message Prism"
          rows={1}
          dir="auto"
          className="flex-1 min-w-0 resize-none bg-transparent pl-3.5 pr-1 py-[9px] text-[12.5px] text-white outline-none max-h-20 leading-snug"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              send(inputValue);
            }
          }}
        />
        <motion.button
          type="submit"
          className="m-[5px] w-[28px] h-[28px] rounded-full flex items-center justify-center flex-shrink-0 transition-opacity disabled:opacity-25"
          style={{ background: "var(--pillar-accent)", color: "var(--pillar-accent-contrast)" }}
          disabled={!canSend}
          aria-label="Send message"
          whileTap={{ scale: 0.86 }}
        >
          <ArrowUpIcon size={15} strokeWidth={2.8} />
        </motion.button>
      </form>
    </div>
  );
}
